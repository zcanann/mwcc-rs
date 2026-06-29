// `a*c1 + a*c2` (or `-`) on the same variable distributes to `a*(c1±c2)`. mwcc applies this
// fold ONLY in a narrow shape — both factors ODD and >= 3 (each its own `mulli`), distinct,
// with the combined factor a power of two — so the two multiplies collapse to a single `slwi`.
// `a*3 + a*5` is `slwi r3,r3,3` (a*8), not `mulli; mulli; add`. An EVEN factor (already
// shift-cheap), a factor of 1 (really `a`), identical terms (CSE'd), or a non-power-of-two
// sum (a `mulli` result) are NOT folded — they keep their existing lowering.
int fold_8(int a)   { return a * 3 + a * 5; }    // a*8  -> slwi r3,r3,3
int fold_16(int a)  { return a * 7 + a * 9; }    // a*16 -> slwi r3,r3,4
int fold_16b(int a) { return a * 3 + a * 13; }   // a*16
int fold_2(int a)   { return a * 5 - a * 3; }    // a*2
int fold_sub(int a) { return a * 13 - a * 5; }   // a*8
int fold_cl(int a)  { return 3 * a + 5 * a; }    // constant on the left
