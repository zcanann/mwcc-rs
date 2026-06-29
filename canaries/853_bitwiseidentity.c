// Bitwise identities collapse to the operand with no instruction: `x | 0`, `x ^ 0`, and
// `x & ~0` (all bits set) are all just `x`. ours had emitted a dead `ori`/`xori`/`rlwinm`;
// mwcc emits nothing (the value stays in its register). These appear in macro-expanded code
// (`FLAGS | NONE` with NONE==0, `x & FULL_MASK`). The Add/Multiply identities were already
// handled; these complete the bitwise set.
int or_zero(int a)      { return a | 0; }           // -> a, no op
int xor_zero(int a)     { return a ^ 0; }           // -> a
int and_all_ones(int a) { return a & 0xffffffff; }  // -> a (all 32 bits kept)
int and_neg_one(int a)  { return a & -1; }          // -> a
int or_zero_expr(int a) { return (a + 1) | 0; }     // identity of a computed value -> a+1
