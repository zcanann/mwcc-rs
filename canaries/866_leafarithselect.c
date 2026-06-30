// `(cond) ? <leaf> : <arithmetic>` — return a cached leaf, else a simple computation. The guard
// form `if (a < 0) return b; return a + 1;` compiles to `cmpwi r3,0; addi r3,r3,1; bgelr; mr
// r3,r4`: compute the false arm into the destination, return it on the false branch, move the
// true leaf over for the true path. The computed arm is restricted to a SIMPLE ARITHMETIC
// expression (add/sub/mul/shift/bitwise/negate) — a comparison, load, call, or cast arm uses
// different codegen and is left to defer (so the computed-arm branch selects never emit wrong
// bytes for those shapes).
int leaf_add(int a, int b)            { return (a < 0) ? b : a + 1; }
int leaf_mul(int a, int b)            { if (a > 0) return b; return a * 2; }
int leaf_xor(int a, int b)            { return (a < 0) ? b : a ^ 3; }
int leaf_neg(int a, int b)            { return (a < 0) ? b : -a; }
int leaf_diffvar(int a, int b, int c) { return (a < 0) ? b : c + 1; }
