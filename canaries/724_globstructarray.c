// Reading a member through a GLOBAL struct ARRAY: materialize the array address
// and scale the index by the struct stride with the interleaved base/scale
// schedule (large array: `lis hi,arr@ha; slwi r0,i,k; addi d,hi,arr@lo`), then
// `lwzx` (offset 0) or `add; lwz offset`. A constant index folds index*stride +
// offset into the displacement. Power-of-two struct strides; the game code's
// fixed-size state tables match this.
struct Gsa { int first; int second; };
struct Gsa gsa_table[16];
int gsa_var(int i)  { return gsa_table[i].second; }
int gsa_first(int i){ return gsa_table[i].first; }
int gsa_const(void) { return gsa_table[3].second; }
