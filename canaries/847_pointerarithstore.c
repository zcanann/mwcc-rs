// `*(p + i) = v` and `*(p + 3) = v` are exactly `p[i] = v` and `p[3] = v` — the store
// counterpart of the *(p+i) load routing. emit_store rewrites a pointer-plus-index
// dereference target to the subscript store (pointer operand = base, integer = index; `+`
// commutes). Unlike the LOAD, the narrow case is fine here: the store truncates the value via
// stb/sth, so there is no sign-extension hazard — every pointee width is byte-exact.
void store_index(int* p, int i, int x)    { *(p + i) = x; }   // slwi; stwx
void store_const(int* p, int x)           { *(p + 3) = x; }   // stw x,12(p)
void store_commuted(int* p, int i, int x) { *(i + p) = x; }   // same as *(p + i)
void store_char(char* p, int i, int x)    { *(p + i) = x; }   // stbx (value truncated)
void store_short(short* p, int i, short x){ *(p + i) = x; }   // sthx
