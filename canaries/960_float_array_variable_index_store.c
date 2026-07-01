// The store counterpart of 959: a VARIABLE-index float/double global-array store (`fa[i] = x`).
// The value stays in its FPR (f1); the index scales into the scratch (`slwi r0,i,2`) and its
// register is reused as the GPR base (a float value doesn't occupy a GPR). The store is
// `stfsx`/`stfdx`:
//   small : slwi r0,i,2; li  b,g@sda21;               stfsx f1,b,r0
//   large : slwi r0,i,2; lis h,g@ha; addi b,h,g@l;     stfsx f1,b,r0   (h avoids the index)
float  fs[2];   // small (SDA21)
float  fl[4];   // large (ADDR16)
double dl[4];   // large (ADDR16)

void small_st(int i, float x)  { fs[i] = x; }   // slwi r0,r3,2; li r3,fs@sda21; stfsx f1,r3,r0
void large_st(int i, float x)  { fl[i] = x; }   // lis r4,fl@ha; slwi r0,r3,2; addi r3,r4,fl@l; stfsx
void dbl_st(int i, double x)   { dl[i] = x; }   // slwi r0,r3,3; ...; stfdx f1,r3,r0
