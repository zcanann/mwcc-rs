// A VARIABLE rotate `(a << n) | (a >> (32 - n))` on an UNSIGNED value folds to a single `rotlw`
// (`rlwnm d,a,n,0,31`). The mirror right rotate `(a >> n) | (a << (32 - n))` computes the amount
// first (`subfic r0,n,32; rotlw d,a,r0`). The rotated value must be unsigned — a signed `a >> Q` is
// an arithmetic shift, not a rotate, and defers (its literal shift-or has an unmodeled schedule).
// Constant-amount rotates go through the rlwimi field-merge path instead. (fire 595)
unsigned rotl(unsigned a, int n)      { return (a << n) | (a >> (32 - n)); }  // rotlw r3,r3,r4
unsigned rotr(unsigned a, int n)      { return (a >> n) | (a << (32 - n)); }  // subfic r0,r4,32; rotlw r3,r3,r0
unsigned rotl_uns(unsigned a, unsigned n) { return (a << n) | (a >> (32 - n)); }
unsigned rotl_commuted(unsigned x, int s) { return (x >> (32 - s)) | (x << s); }  // same rotl, OR commuted
