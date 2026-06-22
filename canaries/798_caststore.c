// int->float/double conversion into a *store*: the magic bias double must go in a
// register distinct from the assembled value's f0 (FLOAT_SCRATCH). Ours put the bias
// in the destination, which is f0 for a store — so the assembled `lfd f0` overwrote
// it and `fsub f0,f0,f0` produced 0. mwcc keeps the bias in f1 (`lfd f1,bias; ...;
// lfd f0,8(r1); fsub f0,f0,f1`). The return form (result in f1) was already correct.
double gd; float gf;
void id(int a)        { gd = (double)a; }    // lfd f1,bias; fsub f0,f0,f1
void iff(int a)       { gf = (float)a; }     // fsubs
void ud(unsigned a)   { gd = (double)a; }    // unsigned bias 0x4330000000000000
double rid(int a)     { return (double)a; }  // result in f1 — bias also f1, unchanged
