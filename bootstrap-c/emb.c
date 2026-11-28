#include "emb.h"

#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * `.emb` v2 loader. This mirrors `bytecode::deserialize` in src/bytecode.rs:
 * the same wire layout, the same validation rules, and the same error text.
 *
 * Both entry points return 0 on success and -1 on failure. On failure `*out`
 * is set to NULL and `*err` receives a heap-allocated message that the caller
 * must free. `err` should point to a NULL-initialised `char *`; on success it
 * is left untouched. `em_emb_free` accepts NULL and releases everything owned
 * by a successfully parsed program.
 */

/* ------------------------------------------------------------------------- */
/* Errors                                                                    */
/* ------------------------------------------------------------------------- */

static void set_error(char **err, const char *fmt, ...)
{
    va_list ap;
    int needed;
    char *msg;

    if (err == NULL) {
        return;
    }
    va_start(ap, fmt);
    needed = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (needed < 0) {
        return;
    }
    msg = (char *)malloc((size_t)needed + 1);
    if (msg == NULL) {
        return;
    }
    va_start(ap, fmt);
    (void)vsnprintf(msg, (size_t)needed + 1, fmt, ap);
    va_end(ap);
    free(*err);
    *err = msg;
}

/* ------------------------------------------------------------------------- */
/* Growing vectors                                                           */
/* ------------------------------------------------------------------------- */

typedef struct {
    void *data;
    size_t count;
    size_t cap;
} Vec;

static int vec_reserve(Vec *v, size_t elem, size_t extra, char **err)
{
    size_t need;
    size_t cap;
    void *data;

    if (extra > SIZE_MAX - v->count || elem == 0 || extra == 0) {
        set_error(err, "Out of memory");
        return -1;
    }
    need = v->count + extra;
    if (need <= v->cap) {
        return 0;
    }
    cap = v->cap == 0 ? 8 : v->cap;
    while (cap < need) {
        if (cap > SIZE_MAX / 2) {
            cap = need;
            break;
        }
        cap *= 2;
    }
    if (cap > SIZE_MAX / elem) {
        set_error(err, "Out of memory");
        return -1;
    }
    data = realloc(v->data, cap * elem);
    if (data == NULL) {
        set_error(err, "Out of memory");
        return -1;
    }
    v->data = data;
    v->cap = cap;
    return 0;
}

static int vec_push(Vec *v, size_t elem, const void *item, char **err)
{
    if (vec_reserve(v, elem, 1, err) != 0) {
        return -1;
    }
    memcpy((char *)v->data + v->count * elem, item, elem);
    v->count++;
    return 0;
}

static void vec_free_strings(Vec *v)
{
    size_t i;
    for (i = 0; i < v->count; i++) {
        free(((char **)v->data)[i]);
    }
    free(v->data);
}

/* ------------------------------------------------------------------------- */
/* UTF-8 validation (String::from_utf8 rejects the same sequences)           */
/* ------------------------------------------------------------------------- */

static bool is_cont(uint8_t b)
{
    return (b & 0xc0u) == 0x80u;
}

static bool valid_utf8(const uint8_t *s, size_t n)
{
    size_t i = 0;

    while (i < n) {
        uint8_t c = s[i];
        size_t extra;
        size_t j;

        if (c < 0x80u) {
            i++;
            continue;
        }
        if (c >= 0xc2u && c <= 0xdfu) {
            extra = 1;
        } else if (c == 0xe0u) {
            if (i + 1 >= n || s[i + 1] < 0xa0u || s[i + 1] > 0xbfu) {
                return false;
            }
            extra = 2;
        } else if ((c >= 0xe1u && c <= 0xecu) || (c >= 0xeeu && c <= 0xefu)) {
            extra = 2;
        } else if (c == 0xedu) {
            if (i + 1 >= n || s[i + 1] < 0x80u || s[i + 1] > 0x9fu) {
                return false; /* UTF-16 surrogates are not scalar values */
            }
            extra = 2;
        } else if (c == 0xf0u) {
            if (i + 1 >= n || s[i + 1] < 0x90u || s[i + 1] > 0xbfu) {
                return false;
            }
            extra = 3;
        } else if (c >= 0xf1u && c <= 0xf3u) {
            extra = 3;
        } else if (c == 0xf4u) {
            if (i + 1 >= n || s[i + 1] < 0x80u || s[i + 1] > 0x8fu) {
                return false;
            }
            extra = 3;
        } else {
            return false;
        }
        if (extra > n - i - 1) {
            return false;
        }
        for (j = 1; j <= extra; j++) {
            if (!is_cont(s[i + j])) {
                return false;
            }
        }
        i += extra + 1;
    }
    return true;
}

/* ------------------------------------------------------------------------- */
/* Little-endian reader                                                      */
/* ------------------------------------------------------------------------- */

typedef struct {
    const uint8_t *bytes;
    size_t len;
    size_t pos;
} Reader;

static int reader_take(Reader *r, size_t n, const uint8_t **out, char **err)
{
    if (n > r->len - r->pos) {
        set_error(err, "Corrupt bytecode: unexpected end of file");
        return -1;
    }
    *out = r->bytes + r->pos;
    r->pos += n;
    return 0;
}

static int reader_u8(Reader *r, uint8_t *out, char **err)
{
    const uint8_t *raw;
    if (reader_take(r, 1, &raw, err) != 0) {
        return -1;
    }
    *out = raw[0];
    return 0;
}

static int reader_u16(Reader *r, uint16_t *out, char **err)
{
    const uint8_t *raw;
    if (reader_take(r, 2, &raw, err) != 0) {
        return -1;
    }
    *out = (uint16_t)((uint16_t)raw[0] | ((uint16_t)raw[1] << 8));
    return 0;
}

static int reader_u32(Reader *r, uint32_t *out, char **err)
{
    const uint8_t *raw;
    if (reader_take(r, 4, &raw, err) != 0) {
        return -1;
    }
    *out = (uint32_t)raw[0] | ((uint32_t)raw[1] << 8) |
           ((uint32_t)raw[2] << 16) | ((uint32_t)raw[3] << 24);
    return 0;
}

/* A NUL-terminated copy; embedded NUL bytes are preserved in the allocation. */
static char *reader_string(Reader *r, char **err)
{
    uint32_t len;
    const uint8_t *raw;
    char *copy;

    if (reader_u32(r, &len, err) != 0) {
        return NULL;
    }
    if (reader_take(r, (size_t)len, &raw, err) != 0) {
        return NULL;
    }
    if (!valid_utf8(raw, (size_t)len)) {
        set_error(err, "Corrupt bytecode: invalid UTF-8 string");
        return NULL;
    }
    copy = (char *)malloc((size_t)len + 1);
    if (copy == NULL) {
        set_error(err, "Out of memory");
        return NULL;
    }
    memcpy(copy, raw, (size_t)len);
    copy[len] = '\0';
    return copy;
}

/* ------------------------------------------------------------------------- */
/* Ownership cleanup                                                         */
/* ------------------------------------------------------------------------- */

typedef struct {
    Vec constants;
    Vec functions;
    Vec code;
} ChunkBuild;

static void proto_cleanup(EmFunctionProto *proto);

static void chunk_cleanup(EmChunk *chunk)
{
    size_t i;

    if (chunk == NULL) {
        return;
    }
    for (i = 0; i < chunk->nconstants; i++) {
        if (chunk->constants[i].tag == EMC_STRING) {
            free(chunk->constants[i].as.string);
        }
    }
    free(chunk->constants);
    chunk->constants = NULL;
    chunk->nconstants = 0;

    for (i = 0; i < chunk->nfunctions; i++) {
        proto_cleanup(&chunk->functions[i]);
    }
    free(chunk->functions);
    chunk->functions = NULL;
    chunk->nfunctions = 0;

    free(chunk->code);
    chunk->code = NULL;
    chunk->ncode = 0;
}

static void proto_cleanup(EmFunctionProto *proto)
{
    size_t i;

    if (proto == NULL) {
        return;
    }
    free(proto->name);
    for (i = 0; i < proto->nparams; i++) {
        free(proto->params[i]);
    }
    free(proto->params);
    if (proto->chunk != NULL) {
        chunk_cleanup(proto->chunk);
        free(proto->chunk);
    }
    memset(proto, 0, sizeof(*proto));
}

static void build_cleanup(ChunkBuild *b)
{
    size_t i;

    for (i = 0; i < b->constants.count; i++) {
        if (((EmConst *)b->constants.data)[i].tag == EMC_STRING) {
            free(((EmConst *)b->constants.data)[i].as.string);
        }
    }
    free(b->constants.data);
    for (i = 0; i < b->functions.count; i++) {
        proto_cleanup(&((EmFunctionProto *)b->functions.data)[i]);
    }
    free(b->functions.data);
    free(b->code.data);
    memset(b, 0, sizeof(*b));
}

/* ------------------------------------------------------------------------- */
/* Validation                                                                */
/* ------------------------------------------------------------------------- */

static bool is_name_op(uint8_t tag)
{
    switch (tag) {
    case OP_GET_VAR:
    case OP_SET_VAR:
    case OP_DEFINE_VAR:
    case OP_GET_PROP:
    case OP_SET_PROP:
    case OP_GET_VAR_OR_OBJECT:
    case OP_GET_PROP_OR_OBJECT:
    case OP_GET_PROP_KEEP:
    case OP_IMPORT:
    case OP_METHOD_CALL:
    case OP_ITER_NEXT:
    case OP_CALL:
        return true;
    default:
        return false;
    }
}

static bool is_jump_op(uint8_t tag)
{
    switch (tag) {
    case OP_JUMP:
    case OP_JUMP_IF_FALSE:
    case OP_JUMP_IF_FALSE_PEEK:
    case OP_JUMP_IF_TRUE_PEEK:
    case OP_JUMP_IF_NULL_PEEK:
    case OP_REPEAT_NEXT:
    case OP_ITER_NEXT:
        return true;
    default:
        return false;
    }
}

static int validate(ChunkBuild *b, char **err)
{
    EmConst *consts = (EmConst *)b->constants.data;
    size_t nconsts = b->constants.count;
    size_t nfuncs = b->functions.count;
    size_t ncode = b->code.count;
    size_t i;

    for (i = 0; i < ncode; i++) {
        EmInstr *ins = &((EmInstr *)b->code.data)[i];

        /* The name an instruction names must exist and be a string. */
        if (is_name_op(ins->tag)) {
            uint32_t idx = ins->tag == OP_CALL ? ins->b : ins->a;
            if ((size_t)idx >= nconsts || consts[idx].tag != EMC_STRING) {
                set_error(err, "Corrupt bytecode: name index out of range");
                return -1;
            }
        }

        /* Jump targets are absolute; one past the end means "done". */
        if (is_jump_op(ins->tag)) {
            uint32_t target = ins->tag == OP_ITER_NEXT ? ins->b : ins->a;
            if ((size_t)target > ncode) {
                set_error(err, "Corrupt bytecode: jump target out of range");
                return -1;
            }
        }

        switch (ins->tag) {
        case OP_LOAD_CONST:
            if ((size_t)ins->a >= nconsts) {
                set_error(err, "Corrupt bytecode: constant index out of range");
                return -1;
            }
            break;
        case OP_MAKE_CLOSURE:
            if ((size_t)ins->a >= nfuncs) {
                set_error(err, "Corrupt bytecode: function index out of range");
                return -1;
            }
            break;
        case OP_ROTATE:
            if (ins->a > EMB_MAX_ROTATE) {
                set_error(err, "Corrupt bytecode: rotate count out of range");
                return -1;
            }
            break;
        default:
            break;
        }
    }
    return 0;
}

/* ------------------------------------------------------------------------- */
/* Chunk decoding                                                            */
/* ------------------------------------------------------------------------- */

static int read_chunk(Reader *r, unsigned depth, EmChunk *out, char **err)
{
    ChunkBuild b;
    uint32_t count;
    uint32_t i;

    memset(&b, 0, sizeof(b));
    if (depth > EMB_MAX_CHUNK_DEPTH) {
        set_error(err, "Corrupt bytecode: functions are nested too deeply");
        return -1;
    }

    /* Constants. */
    if (reader_u32(r, &count, err) != 0) {
        goto fail;
    }
    for (i = 0; i < count; i++) {
        uint8_t tag;
        EmConst value;

        memset(&value, 0, sizeof(value));
        if (reader_u8(r, &tag, err) != 0) {
            goto fail;
        }
        switch (tag) {
        case EMC_NUMBER: {
            const uint8_t *raw;
            uint64_t bits = 0;
            size_t j;

            if (reader_take(r, 8, &raw, err) != 0) {
                goto fail;
            }
            for (j = 0; j < 8; j++) {
                bits |= (uint64_t)raw[j] << (8u * j);
            }
            memcpy(&value.as.number, &bits, sizeof(value.as.number));
            value.tag = EMC_NUMBER;
            break;
        }
        case EMC_STRING:
            value.as.string = reader_string(r, err);
            if (value.as.string == NULL) {
                goto fail;
            }
            value.tag = EMC_STRING;
            break;
        case EMC_BOOL: {
            uint8_t raw;
            if (reader_u8(r, &raw, err) != 0) {
                goto fail;
            }
            value.as.boolean = raw != 0;
            value.tag = EMC_BOOL;
            break;
        }
        case EMC_NULL:
            value.tag = EMC_NULL;
            break;
        default:
            set_error(err, "Corrupt bytecode: unknown constant tag %u", (unsigned)tag);
            goto fail;
        }
        if (vec_push(&b.constants, sizeof(value), &value, err) != 0) {
            if (value.tag == EMC_STRING) {
                free(value.as.string);
            }
            goto fail;
        }
    }

    /* Nested function prototypes. */
    if (reader_u32(r, &count, err) != 0) {
        goto fail;
    }
    for (i = 0; i < count; i++) {
        EmFunctionProto proto;
        Vec params;
        uint32_t nparams;
        uint32_t j;

        memset(&proto, 0, sizeof(proto));
        memset(&params, 0, sizeof(params));
        proto.name = reader_string(r, err);
        if (proto.name == NULL) {
            goto proto_fail;
        }
        if (reader_u32(r, &nparams, err) != 0) {
            goto proto_fail;
        }
        for (j = 0; j < nparams; j++) {
            char *param = reader_string(r, err);
            if (param == NULL) {
                goto proto_fail;
            }
            if (vec_push(&params, sizeof(param), &param, err) != 0) {
                free(param);
                goto proto_fail;
            }
        }
        proto.params = (char **)params.data;
        proto.nparams = params.count;
        memset(&params, 0, sizeof(params)); /* owned by proto now */

        proto.chunk = (EmChunk *)calloc(1, sizeof(*proto.chunk));
        if (proto.chunk == NULL) {
            set_error(err, "Out of memory");
            goto proto_fail;
        }
        if (read_chunk(r, depth + 1, proto.chunk, err) != 0) {
            goto proto_fail;
        }
        if (vec_push(&b.functions, sizeof(proto), &proto, err) != 0) {
            goto proto_fail;
        }
        continue;

    proto_fail:
        vec_free_strings(&params);
        proto_cleanup(&proto);
        goto fail;
    }

    /* Instructions and their source spans. */
    if (reader_u32(r, &count, err) != 0) {
        goto fail;
    }
    for (i = 0; i < count; i++) {
        EmInstr ins;

        if (reader_u8(r, &ins.tag, err) != 0 ||
            reader_u32(r, &ins.a, err) != 0 ||
            reader_u32(r, &ins.b, err) != 0 ||
            reader_u32(r, &ins.line, err) != 0 ||
            reader_u32(r, &ins.col, err) != 0) {
            goto fail;
        }
        if (ins.tag > OP_JUMP_IF_NULL_PEEK) {
            set_error(err, "Corrupt bytecode: unknown instruction %u", (unsigned)ins.tag);
            goto fail;
        }
        if (vec_push(&b.code, sizeof(ins), &ins, err) != 0) {
            goto fail;
        }
    }

    if (validate(&b, err) != 0) {
        goto fail;
    }

    out->constants = (EmConst *)b.constants.data;
    out->nconstants = b.constants.count;
    out->functions = (EmFunctionProto *)b.functions.data;
    out->nfunctions = b.functions.count;
    out->code = (EmInstr *)b.code.data;
    out->ncode = b.code.count;
    return 0;

fail:
    build_cleanup(&b);
    return -1;
}

/* ------------------------------------------------------------------------- */
/* Public API                                                                */
/* ------------------------------------------------------------------------- */

int em_emb_parse_bytes(const uint8_t *bytes, size_t len, EmProgram **out, char **err)
{
    static const uint8_t empty[1] = { 0 };
    Reader r;
    const uint8_t *magic;
    uint16_t version;
    EmProgram *program;

    if (out == NULL) {
        set_error(err, "Invalid argument: null output pointer");
        return -1;
    }
    *out = NULL;
    if (bytes == NULL && len != 0) {
        set_error(err, "Invalid argument: null bytecode buffer");
        return -1;
    }
    if (bytes == NULL) {
        bytes = empty;
    }

    r.bytes = bytes;
    r.len = len;
    r.pos = 0;

    if (reader_take(&r, 4, &magic, err) != 0) {
        return -1;
    }
    if (memcmp(magic, EMB_MAGIC, 4) != 0) {
        set_error(err, "Not an Englishmen bytecode file");
        return -1;
    }
    if (reader_u16(&r, &version, err) != 0) {
        return -1;
    }
    if (version != EMB_VERSION) {
        set_error(err,
                  "Unsupported bytecode version %u (this build of Englishmen speaks version %d)",
                  (unsigned)version, EMB_VERSION);
        return -1;
    }

    program = (EmProgram *)calloc(1, sizeof(*program));
    if (program == NULL) {
        set_error(err, "Out of memory");
        return -1;
    }
    if (read_chunk(&r, 0, &program->root, err) != 0) {
        free(program);
        return -1;
    }
    *out = program;
    return 0;
}

int em_emb_parse_file(const char *path, EmProgram **out, char **err)
{
    FILE *file;
    Vec bytes;
    uint8_t chunk[8192];
    size_t got;
    int result;

    if (out == NULL) {
        set_error(err, "Invalid argument: null output pointer");
        return -1;
    }
    *out = NULL;
    if (path == NULL) {
        set_error(err, "Invalid argument: null bytecode path");
        return -1;
    }

    file = fopen(path, "rb");
    if (file == NULL) {
        set_error(err, "Could not read file '%s': %s", path, strerror(errno));
        return -1;
    }
    memset(&bytes, 0, sizeof(bytes));
    while ((got = fread(chunk, 1, sizeof(chunk), file)) != 0) {
        if (vec_reserve(&bytes, 1, got, err) != 0) {
            free(bytes.data);
            fclose(file);
            return -1;
        }
        memcpy((char *)bytes.data + bytes.count, chunk, got);
        bytes.count += got;
    }
    if (ferror(file)) {
        set_error(err, "Could not read file '%s': %s", path,
                  errno != 0 ? strerror(errno) : "read error");
        free(bytes.data);
        fclose(file);
        return -1;
    }
    fclose(file);

    result = em_emb_parse_bytes((const uint8_t *)bytes.data, bytes.count, out, err);
    free(bytes.data);
    return result;
}

void em_emb_free(EmProgram *program)
{
    if (program == NULL) {
        return;
    }
    chunk_cleanup(&program->root);
    free(program);
}

/* Names match the Rust disassembler's mnemonic column in bytecode.rs. */
const char *em_op_name(uint8_t tag)
{
    static const char *const names[] = {
        "LOAD_CONST",     "POP",            "GET_VAR",        "SET_VAR",
        "DEFINE_VAR",     "GET_PROP",       "SET_PROP",       "GET_VAR_OBJ",
        "GET_PROP_OBJ",   "GET_INDEX",      "SET_INDEX",      "MAKE_ARRAY",
        "MAKE_OBJECT",    "MAKE_CLOSURE",   "CALL",           "METHOD_CALL",
        "SAY",            "ADD",            "SUB",            "MUL",
        "DIV",            "MOD",            "EQ",             "NOT_EQ",
        "GT",             "LT",             "GT_EQ",          "LT_EQ",
        "NEG",            "NOT",            "TO_BOOL",        "JUMP",
        "JUMP_IF_FALSE",  "JUMP_FALSE_PK",  "JUMP_TRUE_PK",   "REPEAT_SETUP",
        "REPEAT_NEXT",    "ITER_SETUP",     "ITER_NEXT",      "RETURN",
        "RETURN_NULL",    "IMPORT",         "FLOOR_DIV",      "POW",
        "BIT_AND",        "BIT_OR",         "BIT_XOR",        "SHL",
        "SHR",            "BIT_NOT",        "DUP",            "ROTATE",
        "GET_PROP_KEEP",  "GET_INDEX_KEEP", "JUMP_NULL_PK"
    };

    if ((size_t)tag >= sizeof(names) / sizeof(names[0])) {
        return "UNKNOWN";
    }
    return names[tag];
}
