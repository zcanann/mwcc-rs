// Signed `x > 0` and `x <= 0` computed into a *store* (result register r0) were
// miscompiles, the same store/scratch collision as the `!= 0` booleanize: the value
// was evaluated into r0, then the idiom's scratch op clobbered it. `x > 0` keeps a
// full-width leaf in its home register for `neg`/`andc`; `x <= 0` reads it with cntlzw
// then puts the `1` in the leaf's freed register. (The `return` form already worked —
// its result is r3, the leaf's register, so no collision.)
int g1, g2, g3, g4;
void gt0(int a)  { g1 = a > 0; }    // neg r0,r3; andc r0,r0,r3; srwi
void le0(int a)  { g2 = a <= 0; }   // cntlzw r0,r3; li r3,1; rlwnm
void lt0(int a)  { g3 = a < 0; }    // unchanged (srwi of the sign bit)
void ge0(int a)  { g4 = a >= 0; }   // unchanged
// Two-operand `a < b` into a store: the `(a^b)>>1` intermediate goes to a fresh
// register (mwcc's `srawi r3`), not the destination — writing it into r0 (the store)
// would clobber the xor result there. (`>` already did this.)
int g5;
void ltb(int a, int b) { g5 = a < b; }   // xor r0,r4,r3; srawi r3,r0,1; and; subf; srwi
