// A constant-amount SHIFT as the LEFT operand of a commutative op with a register-LEAF right is now
// byte-exact: mwcc orders the SHIFT result FIRST (`(a<<2)+b` -> `slwi r0,r3,2; add r3,r0,r4`), the
// source `<<` driving it (a strength-reduced `a*4` stays leaf-first; verified separately). A non-leaf
// right ((a<<2)+(b<<2)), a global, or a memory operand routes through a placement path that keeps the
// swapped order and still DEFERS.
int sl_add(int a, int b) { return (a << 2) + b; }   // slwi r0,r3,2; add r3,r0,r4
int sl_or(int a, int b)  { return (a << 2) | b; }   // or  r3,r0,r4
int sl_and(int a, int b) { return (a << 2) & b; }   // and r3,r0,r4
int sl_xor(int a, int b) { return (a << 2) ^ b; }   // xor r3,r0,r4
int sr_add(int a, int b) { return (a >> 2) + b; }   // srawi; add r3,r0,r4
int sl_mul(int a, int b) { return (a << 2) * b; }   // mullw r3,r0,r4
