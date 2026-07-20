// Legacy debug type graphs are emitted dependency-first. Anonymous typedef
// aggregates remain unnamed, while source-written tags retain their names.
// Struct values and pointers reference the shared type DIE, and source scalar
// spelling survives storage lowering (`unsigned long` is not `unsigned int`).
// builds: GC/1.3.2 GC/2.6
// flags: -char unsigned -sdata 0 -sdata2 0 -O4,p -inline off -sym on

typedef unsigned char u8;
typedef unsigned long u32;

typedef struct named_s {
    u8* bytes;
    int count;
} Named;

typedef struct {
    int value;
    int* values;
} Child;

typedef union {
    void* address;
    long offset;
} Choice;

typedef struct {
    Named named;
    Child child;
    Child* child_pointer;
    u32* words;
    Choice choice;
} Root;

Root root = {{0, 1}, {2, 0}, 0, 0, {0}};
