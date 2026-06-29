// mwcc constant-folds consecutive constant operations `(x OP c1) OP c2` into a single
// instruction `x OP (c1 ⊕ c2)`; ours had emitted both steps. Holds for every associative op
// (and the Add/Subtract mix), with the combined constant strength-reduced by the existing
// single-constant path. A different inner/outer operator, or a non-constant second operand, is
// left alone.
int fold_shr(int a)   { return (a >> 2) >> 3; }       // srawi r3,r3,5
int fold_shl(int a)   { return (a << 2) << 3; }       // slwi  r3,r3,5
unsigned fold_ushr(unsigned a) { return (a >> 4) >> 4; } // srwi r3,r3,8
int fold_add(int a)   { return (a + 3) + 5; }         // addi  r3,r3,8
int fold_addsub(int a){ return a + 10 - 3; }          // addi  r3,r3,7
int fold_subsub(int a){ return a - 10 - 5; }          // addi  r3,r3,-15
int fold_and(int a)   { return (a & 0xf0) & 0x3c; }   // rlwinm (a & 0x30)
int fold_or(int a)    { return (a | 5) | 2; }         // ori   r3,r3,7
int fold_xor(int a)   { return a ^ 5 ^ 3; }           // xori  r3,r3,6
int fold_mul(int a)   { return a * 2 * 3; }           // mulli r3,r3,6
