// Storing to a float/double global-array element — the store counterpart of 956/957. The value
// stays in its FPR (f1); the base materializes into a free GPR, and the store is `stfs`/`stfd`:
//   * small, offset 0       : `stfs f1, g@sda21(r0)`                       (folded, no base reg)
//   * small, non-zero offset : `li b,g@sda21;  stfs f1,off(b)`
//   * large, offset 0        : `lis b,g@ha;    stfs f1,g@l(b)`             (the @l rides the store)
//   * large, non-zero offset : `lis b,g@ha; addi b,b,g@l; stfs f1,off(b)`
float  fs[2];   // 8-byte  -> SDA21 (small)
float  fl[4];   // 16-byte -> ADDR16 (large)
double dl[2];   // 16-byte -> ADDR16 (large)

void small_0(float x)   { fs[0] = x; }   // stfs f1, fs@sda21(r0)
void small_1(float x)   { fs[1] = x; }   // li b,fs@sda21;  stfs f1,4(b)
void large_0(float x)   { fl[0] = x; }   // lis b,fl@ha;    stfs f1,fl@l(b)
void large_2(float x)   { fl[2] = x; }   // lis b,fl@ha; addi b,b,fl@l; stfs f1,8(b)
void dbl_1(double x)    { dl[1] = x; }   // lis b,dl@ha; addi b,b,dl@l; stfd f1,8(b)
