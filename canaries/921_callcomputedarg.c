// A NON-LEAF function calling with a COMPUTED argument: mwcc hoists the leading ALU arg-compute into
// the mflr->LR-store gap (`g(a+b)` -> `add r3,r3,r4; stw r0,20(r1); bl`), so the LR save lands AFTER
// the arg setup. Ours previously did the LR save first (a DIFF for any non-addi compute). The hoist
// now covers all single-cycle ALU arg-computes; a LOAD arg (g(*p)) is NOT hoisted (kept after the
// save), and at most TWO are hoisted (sink3 keeps its third move after the save, canary 765).
extern int g1(int);
extern int g2(int, int);
int c_add(int a, int b)             { return g1(a + b); }    // add;  stw r0
int c_mul(int a, int b)             { return g1(a * b); }    // mullw
int c_sub(int a, int b)             { return g1(a - b); }    // subf
int c_and(int a, int b)             { return g1(a & b); }    // and
int c_shl(int a)                    { return g1(a << 2); }   // slwi
int c_neg(int a)                    { return g1(-a); }       // neg
int c_load(int *p)                  { return g1(*p); }       // lwz NOT hoisted (after the save)
int c_two(int a, int b, int c, int d){ return g2(a + b, c + d); } // both adds hoisted
