// A bare truthiness condition `if (x)` tests x against 0. mwcc picks the compare by
// signedness: a signed int uses `cmpwi`, but a POINTER or UNSIGNED operand uses
// `cmplwi` (unsigned). ours always emitted cmpwi, so a pointer/unsigned condition
// differed by one instruction (0x2c vs 0x28). Now the bare test honors signedness,
// matching the comparison path. (Leaf stores keep this a clean single-compare test.)
void ptr_cond(int *p)             { if (p) *p = 0; }
void uns_cond(unsigned u, int *q) { if (u) *q = 0; }
void int_cond(int c, int *q)      { if (c) *q = 0; }
