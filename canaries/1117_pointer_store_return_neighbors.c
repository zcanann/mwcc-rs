// Byte-exact NEIGHBORS of the store-scheduled-around-return defer (driver.rs). mwcc's list scheduler
// interleaves a POINTER store with the return-value computation only in specific shapes (a `neg`-
// leading `>0`/`!=0` return, or a materialized store value + computed return); those DEFER. Everything
// adjacent stays byte-exact and must NOT be swept into that defer:
//   - a bare-REGISTER pointer store (`*p = a`, direct `stw`) has no materialize slot, so a computed
//     return after it emits in program order (store, then compute).
//   - a bare-leaf return (already in r3) has nothing to hoist.
//   - a `< 0` / `== 0` return leads with srawi/cntlzw (writes the dest directly), which mwcc does NOT
//     hoist over the store — unlike the `neg r0,x`-leading `> 0` / `!= 0` idioms.
int reg_store_computed(int a, int* p)  { *p = a; return a + 1; }   // stw r3,0(r4); addi r3,r3,1
int reg_store_bare(int a, int* p)      { *p = a; return a; }       // stw r3,0(r4) (a already in r3)
int reg_store_signmask(int a, int* p)  { *p = a; return a < 0; }   // stw r3,0(r4); srawi r3,r3,31
int reg_store_nonneg(int a, int* p)    { *p = a; return a >= 0; }  // stw r3,0(r4); srwi r3,r3,31 (no neg)
int reg_store_iszero(int a, int* p)    { *p = a; return a == 0; }  // stw; cntlzw r0,r3; srwi r3,r0,5
// Contrast: `a == 0` (Binary, above) does NOT hoist, but the semantically-equal `!a` (Unary) DOES —
// its leading `cntlzw` hoists over the store — so `*p=a; return !a;` DEFERS (condition C), not here.
