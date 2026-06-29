// `*(p + i)` and `*(p + 3)` are exactly `p[i]` and `p[3]` — mwcc emits the identical indexed
// load (`slwi; lwzx`) or displacement load (`lwz 12(r3)`). A pointer-plus-index dereference is
// routed to the subscript path (the pointer operand is the base, the integer the index; `+`
// commutes so `*(i + p)` works too). dereferenced_width / pointee_of now see through the
// `p + i` pointer, so a NARROW char/short element is handled in full parity with `p[i]`: a
// direct return sign-extends (extsb via is_signed_byte_load), a masking AND stays raw, and an
// arithmetic use defers (the same char-load defer as `p[i] + 1`).
int   deref_index(int* p, int i)    { return *(p + i); }            // slwi r0,r4,2; lwzx r3,r3,r0
int   deref_const(int* p)           { return *(p + 3); }            // lwz r3,12(r3)
int   deref_commuted(int* p, int i) { return *(i + p); }            // same as *(p + i)
float deref_float(float* p, int i)  { return *(p + i); }            // lfsx
int   deref_sum(int* p)             { return *(p + 1) + *(p + 2); }  // two displacement lwz
unsigned deref_uint(unsigned* p, int i) { return *(p + i); }
int   deref_char(char* p, int i)    { return *(p + i); }            // lbzx; extsb (narrow return)
int   deref_char_const(char* p)     { return *(p + 3); }            // lbz 3(p); extsb
int   deref_short(short* p, int i)  { return *(p + i); }            // lhax (sign-extends)
int   deref_char_mask(char* p, int i){ return *(p + i) & 0xf; }     // lbzx; clrlwi (raw, masked)
