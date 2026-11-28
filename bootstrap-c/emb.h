#ifndef EM_BOOTSTRAP_EMB_H
#define EM_BOOTSTRAP_EMB_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define EMB_MAGIC "EMBC"
#define EMB_VERSION 2
#define EMB_MAX_CHUNK_DEPTH 32
#define EMB_MAX_ROTATE 16

typedef enum {
    EMC_NUMBER = 0,
    EMC_STRING = 1,
    EMC_BOOL = 2,
    EMC_NULL = 3
} EmConstTag;

/* Tags mirror bytecode::encode_op in src/bytecode.rs. */
typedef enum {
    OP_LOAD_CONST = 0,
    OP_POP = 1,
    OP_GET_VAR = 2,
    OP_SET_VAR = 3,
    OP_DEFINE_VAR = 4,
    OP_GET_PROP = 5,
    OP_SET_PROP = 6,
    OP_GET_VAR_OR_OBJECT = 7,
    OP_GET_PROP_OR_OBJECT = 8,
    OP_GET_INDEX = 9,
    OP_SET_INDEX = 10,
    OP_MAKE_ARRAY = 11,
    OP_MAKE_OBJECT = 12,
    OP_MAKE_CLOSURE = 13,
    OP_CALL = 14,
    OP_METHOD_CALL = 15,
    OP_SAY = 16,
    OP_ADD = 17,
    OP_SUB = 18,
    OP_MUL = 19,
    OP_DIV = 20,
    OP_MOD = 21,
    OP_EQ = 22,
    OP_NOT_EQ = 23,
    OP_GT = 24,
    OP_LT = 25,
    OP_GT_EQ = 26,
    OP_LT_EQ = 27,
    OP_NEG = 28,
    OP_NOT = 29,
    OP_TO_BOOL = 30,
    OP_JUMP = 31,
    OP_JUMP_IF_FALSE = 32,
    OP_JUMP_IF_FALSE_PEEK = 33,
    OP_JUMP_IF_TRUE_PEEK = 34,
    OP_REPEAT_SETUP = 35,
    OP_REPEAT_NEXT = 36,
    OP_ITER_SETUP = 37,
    OP_ITER_NEXT = 38,
    OP_RETURN = 39,
    OP_RETURN_NULL = 40,
    OP_IMPORT = 41,
    OP_FLOOR_DIV = 42,
    OP_POW = 43,
    OP_BIT_AND = 44,
    OP_BIT_OR = 45,
    OP_BIT_XOR = 46,
    OP_SHL = 47,
    OP_SHR = 48,
    OP_BIT_NOT = 49,
    OP_DUP = 50,
    OP_ROTATE = 51,
    OP_GET_PROP_KEEP = 52,
    OP_GET_INDEX_KEEP = 53,
    OP_JUMP_IF_NULL_PEEK = 54
} EmOp;

typedef struct {
    EmConstTag tag;
    union {
        double number;
        char *string;
        bool boolean;
    } as;
} EmConst;

typedef struct {
    uint8_t tag;
    uint32_t a;
    uint32_t b;
    uint32_t line;
    uint32_t col;
} EmInstr;

typedef struct EmChunk EmChunk;

typedef struct {
    char *name;
    char **params;
    size_t nparams;
    EmChunk *chunk;
} EmFunctionProto;

struct EmChunk {
    EmConst *constants;
    size_t nconstants;
    EmFunctionProto *functions;
    size_t nfunctions;
    EmInstr *code;
    size_t ncode;
};

typedef struct {
    EmChunk root;
} EmProgram;

int em_emb_parse_bytes(const uint8_t *bytes, size_t len, EmProgram **out, char **err);
int em_emb_parse_file(const char *path, EmProgram **out, char **err);
void em_emb_free(EmProgram *program);
const char *em_op_name(uint8_t tag);

#endif
