// `*(p - C)` for a constant C is `p[-C]` — a displacement load at a NEGATIVE offset, the
// counterpart of the `*(p + C)` routing. Subtract does not commute (the pointer is always the
// left operand). Restricted to a constant offset and a non-narrow pointee: a variable
// `*(p - i)` needs a negated, scaled index, and a char/short pointee needs the narrow
// machinery to see through the `p - C` pointer — both keep deferring (honestly).
int      sub_const(int* p)        { return *(p - 3); }   // lwz r3,-12(p)
int      sub_one(int* p)          { return *(p - 1); }   // lwz r3,-4(p)
unsigned sub_uint(unsigned* p)    { return *(p - 2); }   // lwz r3,-8(p)
float    sub_float(float* p)      { return *(p - 1); }   // lfs f1,-4(p)
int      sub_sum(int* p)          { return *(p - 1) + *(p - 2); }

// The STORE `*(p - C) = v` is `p[-C] = v` — a negative-displacement store. Here every pointee
// width works (the store truncates the value via stb/sth, so no narrow restriction is needed),
// unlike the load above.
void store_word(int* p, int x)    { *(p - 1) = x; }      // stw x,-4(p)
void store_byte(char* p, int x)   { *(p - 2) = x; }      // stb x,-2(p)  (value truncated)
void store_half(short* p, short x){ *(p - 1) = x; }      // sth x,-2(p)
void store_zero(int* p)           { *(p - 1) = 0; }      // li 0; stw -4(p)
