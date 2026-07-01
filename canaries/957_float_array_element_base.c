// A float/double global-array element loads into an FPR from a GPR base that gets its OWN free
// register (the FPR number can't be the base GPR). Extends 956 (small offset-0 fold) to every
// remaining case:
//   * small, non-zero offset : `li b,g@sda21;  lfs f,off(b)`
//   * large, offset 0        : `lis b,g@ha;    lfs f,g@l(b)`   (the @l rides the load)
//   * large, non-zero offset : `lis b,g@ha; addi b,b,g@l; lfs f,off(b)`
// (`fa[0]` small still folds to a single `lfs f, fa@sda21(r0)`, see 956.)
float  fs[2];   // 8-byte  -> SDA21 (small)
float  fl[4];   // 16-byte -> ADDR16 (large)
double dl[2];   // 16-byte -> ADDR16 (large)

float  small_nonzero(void) { return fs[1]; }   // li b,fs@sda21;  lfs f1,4(b)
float  large_zero(void)    { return fl[0]; }   // lis b,fl@ha;    lfs f1,fl@l(b)
float  large_nonzero(void) { return fl[2]; }   // lis b,fl@ha; addi b,b,fl@l; lfs f1,8(b)
double dbl_zero(void)      { return dl[0]; }   // lis b,dl@ha;    lfd f1,dl@l(b)
double dbl_nonzero(void)   { return dl[1]; }   // lis b,dl@ha; addi b,b,dl@l; lfd f1,8(b)
