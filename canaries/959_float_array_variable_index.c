// A VARIABLE-index float/double global-array element (`fa[i]`) loads with `lfsx`/`lfdx` from a
// scaled index in the scratch (`slwi r0,i,2`) plus a GPR base. Because the element loads into an
// FPR, the base cannot be the FPR number: it is the LOWEST FREE GPR (the integer-result register
// r3, unused by a float function), independent of which register holds the index — so the form is
// identical whether the index is param-0 (r3) or param-1 (r4):
//   small : slwi r0,i,2; li  b,g@sda21;        lfsx f1,b,r0
//   large : slwi r0,i,2; lis b,g@ha; addi b,b,g@l; lfsx f1,b,r0
// (An integer element instead uses the result register as its base, unchanged.)
float  fs[2];   // small (SDA21)
float  fl[4];   // large (ADDR16)
double dl[4];   // large (ADDR16)

float  small_idx(int i)        { return fs[i]; }        // slwi r0,r3,2; li r3,fs@sda21; lfsx f1,r3,r0
float  large_idx(int i)        { return fl[i]; }        // lis r3,fl@ha; slwi r0,r3,2; addi r3,r3,fl@l; lfsx
float  large_idx_p1(int j, int i) { return fl[i]; }     // index in r4, base still r3
double dbl_idx(int i)          { return dl[i]; }        // slwi r0,r3,3; ...; lfdx f1,r3,r0
