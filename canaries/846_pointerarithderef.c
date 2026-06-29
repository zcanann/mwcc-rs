// `*(p + i)` and `*(p + 3)` are exactly `p[i]` and `p[3]` — mwcc emits the identical indexed
// load (`slwi; lwzx`) or displacement load (`lwz 12(r3)`). A pointer-plus-index dereference
// is now routed to the subscript path (the pointer operand is the base, the integer the
// index; `+` commutes so `*(i + p)` works too). Restricted to NON-narrow pointees
// (int/float/double/pointer): a char/short element needs the load's sign/zero-extension that
// the narrow-load machinery does not yet recognize through the `p + i` pointer, so a narrow
// `*(p+i)` still defers (rather than drop the extsb — a miscompile).
int   deref_index(int* p, int i)    { return *(p + i); }            // slwi r0,r4,2; lwzx r3,r3,r0
int   deref_const(int* p)           { return *(p + 3); }            // lwz r3,12(r3)
int   deref_commuted(int* p, int i) { return *(i + p); }            // same as *(p + i)
float deref_float(float* p, int i)  { return *(p + i); }            // lfsx
int   deref_sum(int* p)             { return *(p + 1) + *(p + 2); }  // two displacement lwz
unsigned deref_uint(unsigned* p, int i) { return *(p + i); }
