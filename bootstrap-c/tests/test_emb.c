#include "emb.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(cond)                                                            \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);    \
            exit(1);                                                           \
        }                                                                      \
    } while (0)

static const char *const OP_NAMES[] = {
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

/* ---- tiny byte buffer writer ------------------------------------------- */

typedef struct {
    uint8_t *data;
    size_t len;
    size_t cap;
} Buf;

static void buf_need(Buf *b, size_t extra)
{
    size_t need = b->len + extra;
    size_t cap;

    if (need <= b->cap) {
        return;
    }
    cap = b->cap == 0 ? 64 : b->cap;
    while (cap < need) {
        cap *= 2;
    }
    b->data = (uint8_t *)realloc(b->data, cap);
    CHECK(b->data != NULL);
    b->cap = cap;
}

static void b_bytes(Buf *b, const void *src, size_t n)
{
    buf_need(b, n);
    memcpy(b->data + b->len, src, n);
    b->len += n;
}

static void b_u8(Buf *b, uint8_t v)
{
    b_bytes(b, &v, 1);
}

static void b_u16(Buf *b, uint16_t v)
{
    b_u8(b, (uint8_t)(v & 0xffu));
    b_u8(b, (uint8_t)((v >> 8) & 0xffu));
}

static void b_u32(Buf *b, uint32_t v)
{
    b_u16(b, (uint16_t)(v & 0xffffu));
    b_u16(b, (uint16_t)((v >> 16) & 0xffffu));
}

static void b_str(Buf *b, const char *s)
{
    size_t n = strlen(s);
    CHECK(n <= UINT32_MAX);
    b_u32(b, (uint32_t)n);
    b_bytes(b, s, n);
}

static void b_header(Buf *b)
{
    b_bytes(b, "EMBC", 4);
    b_u16(b, EMB_VERSION);
}

static void b_num(Buf *b, double value)
{
    uint64_t bits;
    int i;

    memcpy(&bits, &value, sizeof(bits));
    b_u8(b, 0);
    for (i = 0; i < 8; i++) {
        b_u8(b, (uint8_t)((bits >> (8 * i)) & 0xffu));
    }
}

static void b_str_const(Buf *b, const char *s)
{
    b_u8(b, 1);
    b_str(b, s);
}

static void b_bool_const(Buf *b, int value)
{
    b_u8(b, 2);
    b_u8(b, value ? 1 : 0);
}

static void b_null_const(Buf *b)
{
    b_u8(b, 3);
}

static void b_instr(Buf *b, uint8_t tag, uint32_t a, uint32_t c,
                    uint32_t line, uint32_t col)
{
    b_u8(b, tag);
    b_u32(b, a);
    b_u32(b, c);
    b_u32(b, line);
    b_u32(b, col);
}

/* ---- expected operands for the all-ops image --------------------------- */

static uint32_t expected_a(uint8_t tag)
{
    switch (tag) {
    case OP_GET_VAR:
    case OP_SET_VAR:
    case OP_DEFINE_VAR:
    case OP_GET_PROP:
    case OP_SET_PROP:
    case OP_GET_VAR_OR_OBJECT:
    case OP_GET_PROP_OR_OBJECT:
    case OP_IMPORT:
    case OP_METHOD_CALL:
    case OP_ITER_NEXT:
    case OP_GET_PROP_KEEP:
        return 1; /* string constant "x" */
    case OP_ROTATE:
        return 2;
    default:
        return 0;
    }
}

static uint32_t expected_b(uint8_t tag)
{
    return tag == OP_CALL ? 1u : 0u; /* CALL's name is its second operand */
}

/* ---- hand-built images -------------------------------------------------- */

static Buf all_ops_image(void)
{
    static const char *const params[] = { "p", "q" };
    Buf body;
    Buf b;
    uint8_t tag;

    memset(&body, 0, sizeof(body));
    b_u32(&body, 1);
    b_str_const(&body, "y");
    b_u32(&body, 0); /* no functions inside the nested function */
    b_u32(&body, 2);
    b_instr(&body, OP_LOAD_CONST, 0, 0, 10, 1);
    b_instr(&body, OP_RETURN_NULL, 0, 0, 11, 2);

    memset(&b, 0, sizeof(b));
    b_header(&b);
    b_u32(&b, 4); /* number, string, bool, null */
    b_num(&b, 3.5);
    b_str_const(&b, "x");
    b_bool_const(&b, 1);
    b_null_const(&b);
    b_u32(&b, 1); /* one function prototype */
    b_str(&b, "inner");
    b_u32(&b, 2);
    b_str(&b, params[0]);
    b_str(&b, params[1]);
    b_bytes(&b, body.data, body.len);
    b_u32(&b, 55); /* every op tag, 0..54 */
    for (tag = 0; tag <= OP_JUMP_IF_NULL_PEEK; tag++) {
        b_instr(&b, tag, expected_a(tag), expected_b(tag),
                (uint32_t)tag + 1, (uint32_t)tag + 2);
    }

    free(body.data);
    return b;
}

/* A root chunk with two constants ("x", null), no functions, one instruction. */
static Buf one_instr_image(uint8_t tag, uint32_t a, uint32_t c)
{
    Buf b;

    memset(&b, 0, sizeof(b));
    b_header(&b);
    b_u32(&b, 2);
    b_str_const(&b, "x");
    b_null_const(&b);
    b_u32(&b, 0);
    b_u32(&b, 1);
    b_instr(&b, tag, a, c, 7, 9);
    return b;
}

/* ---- error helpers ------------------------------------------------------ */

static void expect_bytes_error(const char *label, const uint8_t *bytes,
                               size_t len, const char *needle)
{
    EmProgram *program = (EmProgram *)(uintptr_t)1;
    char *err = NULL;
    int rc = em_emb_parse_bytes(bytes, len, &program, &err);

    if (rc == 0 || program != NULL || err == NULL ||
        strstr(err, needle) == NULL) {
        fprintf(stderr, "FAIL %s: rc=%d program=%p err=%s (wanted '%s')\n",
                label, rc, (void *)program, err == NULL ? "(null)" : err, needle);
        free(err);
        exit(1);
    }
    free(err);
}

static void expect_file_error(const char *path, const char *needle)
{
    EmProgram *program = (EmProgram *)(uintptr_t)1;
    char *err = NULL;
    int rc = em_emb_parse_file(path, &program, &err);

    CHECK(rc != 0);
    CHECK(program == NULL);
    CHECK(err != NULL);
    CHECK(strstr(err, needle) != NULL);
    free(err);
}

/* ---- tests -------------------------------------------------------------- */

static void test_all_constants_functions_and_ops(void)
{
    Buf img = all_ops_image();
    EmProgram *program = NULL;
    char *err = NULL;
    EmChunk *root;
    EmFunctionProto *fn;
    uint8_t tag;
    size_t i;

    CHECK(em_emb_parse_bytes(img.data, img.len, &program, &err) == 0);
    CHECK(err == NULL);
    CHECK(program != NULL);
    root = &program->root;

    CHECK(root->nconstants == 4);
    CHECK(root->constants[0].tag == EMC_NUMBER);
    CHECK(fabs(root->constants[0].as.number - 3.5) < 0.0001);
    CHECK(root->constants[1].tag == EMC_STRING);
    CHECK(strcmp(root->constants[1].as.string, "x") == 0);
    CHECK(root->constants[2].tag == EMC_BOOL);
    CHECK(root->constants[2].as.boolean == true);
    CHECK(root->constants[3].tag == EMC_NULL);

    CHECK(root->nfunctions == 1);
    fn = &root->functions[0];
    CHECK(strcmp(fn->name, "inner") == 0);
    CHECK(fn->nparams == 2);
    CHECK(strcmp(fn->params[0], "p") == 0);
    CHECK(strcmp(fn->params[1], "q") == 0);
    CHECK(fn->chunk != NULL);
    CHECK(fn->chunk->nconstants == 1);
    CHECK(strcmp(fn->chunk->constants[0].as.string, "y") == 0);
    CHECK(fn->chunk->nfunctions == 0);
    CHECK(fn->chunk->ncode == 2);
    CHECK(fn->chunk->code[0].tag == OP_LOAD_CONST);
    CHECK(fn->chunk->code[0].line == 10 && fn->chunk->code[0].col == 1);
    CHECK(fn->chunk->code[1].tag == OP_RETURN_NULL);
    CHECK(fn->chunk->code[1].line == 11 && fn->chunk->code[1].col == 2);

    CHECK(root->ncode == 55);
    for (i = 0; i < root->ncode; i++) {
        const EmInstr *ins = &root->code[i];
        tag = (uint8_t)i;
        CHECK(ins->tag == tag);
        CHECK(ins->a == expected_a(tag));
        CHECK(ins->b == expected_b(tag));
        CHECK(ins->line == (uint32_t)i + 1);
        CHECK(ins->col == (uint32_t)i + 2);
        CHECK(strcmp(em_op_name(tag), OP_NAMES[tag]) == 0);
    }
    CHECK(strcmp(em_op_name(255), "UNKNOWN") == 0);

    em_emb_free(program);
    em_emb_free(NULL);
    free(img.data);
}

static void test_trailing_bytes_are_ignored(void)
{
    Buf img = all_ops_image();
    EmProgram *program = NULL;
    char *err = NULL;
    char suffix[3] = { 'x', 'y', 'z' };

    b_bytes(&img, suffix, sizeof(suffix));
    CHECK(em_emb_parse_bytes(img.data, img.len, &program, &err) == 0);
    CHECK(err == NULL);
    CHECK(program != NULL);
    CHECK(program->root.ncode == 55);
    em_emb_free(program);
    free(img.data);
}

static void test_malformed_header_and_constants(void)
{
    const char *source = "say(\"hello\")\n";
    Buf short_file;
    Buf bad_version;
    Buf bad_const;
    Buf bad_utf8;
    Buf big_params;
    Buf deep;
    Buf good = all_ops_image();
    uint8_t i;
    uint32_t nested[4] = { 0, 1, 0, 0 }; /* consts, funcs, empty name, params */

    expect_bytes_error("not bytecode", (const uint8_t *)source, strlen(source),
                       "Not an Englishmen bytecode file");

    memset(&short_file, 0, sizeof(short_file));
    b_bytes(&short_file, "EM", 2);
    expect_bytes_error("short magic", short_file.data, short_file.len,
                       "unexpected end of file");

    bad_version = all_ops_image();
    bad_version.data[4] = EMB_VERSION + 1;
    expect_bytes_error("future version", bad_version.data, bad_version.len,
                       "Unsupported bytecode version");
    expect_bytes_error("truncated image", good.data, good.len / 2,
                       "Corrupt bytecode");

    memset(&bad_const, 0, sizeof(bad_const));
    b_header(&bad_const);
    b_u32(&bad_const, 1);
    b_u8(&bad_const, 9);
    expect_bytes_error("unknown constant tag", bad_const.data, bad_const.len,
                       "unknown constant tag 9");

    memset(&bad_utf8, 0, sizeof(bad_utf8));
    b_header(&bad_utf8);
    b_u32(&bad_utf8, 1);
    b_u8(&bad_utf8, 1);
    b_u32(&bad_utf8, 1);
    b_u8(&bad_utf8, 0xff);
    expect_bytes_error("bad utf8", bad_utf8.data, bad_utf8.len,
                       "invalid UTF-8 string");

    memset(&big_params, 0, sizeof(big_params));
    b_header(&big_params);
    b_u32(&big_params, 0); /* no constants */
    b_u32(&big_params, 1); /* one function */
    b_u32(&big_params, 0); /* empty name */
    b_u32(&big_params, UINT32_MAX);
    expect_bytes_error("huge parameter count", big_params.data, big_params.len,
                       "unexpected end of file");

    memset(&deep, 0, sizeof(deep));
    b_header(&deep);
    for (i = 0; i < 40; i++) {
        b_bytes(&deep, nested, sizeof(nested));
    }
    expect_bytes_error("deep nesting", deep.data, deep.len, "nested too deeply");

    free(short_file.data);
    free(bad_version.data);
    free(bad_const.data);
    free(bad_utf8.data);
    free(big_params.data);
    free(deep.data);
    free(good.data);
}

static void test_malformed_instructions(void)
{
    Buf img;

    img = one_instr_image(OP_GET_VAR, 999, 0);
    expect_bytes_error("name out of range", img.data, img.len,
                       "name index out of range");
    free(img.data);

    img = one_instr_image(OP_GET_VAR, 1, 0); /* const 1 is null, not a name */
    expect_bytes_error("name is not a string", img.data, img.len,
                       "name index out of range");
    free(img.data);

    img = one_instr_image(OP_CALL, 0, 999);
    expect_bytes_error("call name out of range", img.data, img.len,
                       "name index out of range");
    free(img.data);

    img = one_instr_image(OP_JUMP, 999, 0);
    expect_bytes_error("jump out of range", img.data, img.len,
                       "jump target out of range");
    free(img.data);

    img = one_instr_image(OP_ITER_NEXT, 0, 999);
    expect_bytes_error("iter jump out of range", img.data, img.len,
                       "jump target out of range");
    free(img.data);

    img = one_instr_image(OP_LOAD_CONST, 999, 0);
    expect_bytes_error("const out of range", img.data, img.len,
                       "constant index out of range");
    free(img.data);

    img = one_instr_image(OP_MAKE_CLOSURE, 999, 0);
    expect_bytes_error("function out of range", img.data, img.len,
                       "function index out of range");
    free(img.data);

    img = one_instr_image(OP_ROTATE, EMB_MAX_ROTATE + 1, 0);
    expect_bytes_error("rotate out of range", img.data, img.len,
                       "rotate count out of range");
    free(img.data);

    img = one_instr_image(255, 0, 0);
    expect_bytes_error("unknown instruction", img.data, img.len,
                       "unknown instruction 255");
    free(img.data);

    /* Boundary cases: one past the last instruction is a legal jump target,
       and the largest accepted rotate count must load. */
    img = one_instr_image(OP_JUMP, 1, 0);
    {
        EmProgram *program = NULL;
        char *err = NULL;
        CHECK(em_emb_parse_bytes(img.data, img.len, &program, &err) == 0);
        CHECK(err == NULL);
        em_emb_free(program);
    }
    free(img.data);

    img = one_instr_image(OP_ROTATE, EMB_MAX_ROTATE, 0);
    {
        EmProgram *program = NULL;
        char *err = NULL;
        CHECK(em_emb_parse_bytes(img.data, img.len, &program, &err) == 0);
        CHECK(err == NULL);
        em_emb_free(program);
    }
    free(img.data);
}

static void test_parse_file(void)
{
    static const char *path = "test_emb_loader_tmp.emb";
    static const char *missing = "test_emb_loader_no_such_file.emb";
    Buf img = all_ops_image();
    FILE *file = fopen(path, "wb");
    EmProgram *program = (EmProgram *)(uintptr_t)1;
    char *err = NULL;

    CHECK(file != NULL);
    CHECK(fwrite(img.data, 1, img.len, file) == img.len);
    CHECK(fclose(file) == 0);

    CHECK(em_emb_parse_file(path, &program, &err) == 0);
    CHECK(err == NULL);
    CHECK(program != NULL);
    CHECK(program->root.ncode == 55);
    em_emb_free(program);

    expect_file_error(missing, "Could not read file");
    CHECK(remove(path) == 0);
    free(img.data);
}

int main(void)
{
    CHECK(sizeof(OP_NAMES) / sizeof(OP_NAMES[0]) ==
          (size_t)OP_JUMP_IF_NULL_PEEK + 1);
    test_all_constants_functions_and_ops();
    test_trailing_bytes_are_ignored();
    test_malformed_header_and_constants();
    test_malformed_instructions();
    test_parse_file();
    puts("emb tests passed");
    return 0;
}
