// C's usual arithmetic conversions make a relational comparison UNSIGNED if EITHER operand is
// unsigned: `int a < unsigned b` converts a to unsigned and compares unsigned, so mwcc emits
// its unsigned branchless idiom (xor;cntlzw;slw;srwi), NOT the signed srawi form. ours had
// keyed the idiom off the LEFT operand only, so `int a < unsigned b` wrongly used the signed
// idiom (a MISCOMPILE: a=-1 compares as 0xFFFFFFFF, the largest unsigned, not the smallest).
// `==`/`!=` are signedness-INDEPENDENT (bit patterns differ iff values do); mwcc keys their
// idiom off the left operand's declared type, so they keep the left's signedness.
int lt_iu(int a, unsigned b)      { return a < b; }   // unsigned idiom (b unsigned)
int le_iu(int a, unsigned b)      { return a <= b; }
int gt_iu(int a, unsigned b)      { return a > b; }
int ge_iu(int a, unsigned b)      { return a >= b; }
int eq_iu(int a, unsigned b)      { return a == b; }  // signedness-independent
int ne_iu(int a, unsigned b)      { return a != b; }
int lt_ui(unsigned a, int b)      { return a < b; }   // unsigned (a unsigned)
int lt_ss(int a, int b)           { return a < b; }   // signed (both signed)
int lt_uu(unsigned a, unsigned b) { return a < b; }   // unsigned (both)
