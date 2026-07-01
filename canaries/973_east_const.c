// Two fixes exercised here:
// (1) EAST-CONST: a `const`/`volatile` qualifier may TRAIL the base type (`int const x`
//     == `const int x`); dolphin/MSL headers use both (e.g. `static float const
//     deg_to_rad` in MSL math.h). Previously failed to parse ("expected ParenOpen").
// (2) CONSTANT-LOCAL FOLD: a single local initialized to a constant is inlined at its
//     use, matching mwcc's fold (`int k=3; return x+k` -> `addi r3,r3,3`) rather than
//     materializing it (`li r0,3; add`). Value-tracking now takes over this shape.
int const gci = 7;

int read_const_int(void)      { return gci; }              // east-const int global (folded)
int fold_add(int x)           { int k = 3; return x + k; } // constant-local fold -> addi
int fold_mul(int x)           { int k = 3; return x * k; } // -> mulli
int fold_sub(int x)           { int const k = 5; return x - k; } // east-const local + fold
int fold_ret(void)            { int k = 42; return k; }    // -> li r3,42
