// The init-INTERLEAVE schedule over a narrow condition — the first modeled slice of the __va_arg
// multi-local drive (its type-test shape). An UNSIGNED narrow parameter compared against a small
// constant, guarding a const-init/const-reassign local returned bare: mwcc widens into the scratch,
// fills the width-op -> compare latency gap with the local's INITIALIZER (loaded straight into the
// result register), then the LOGICAL compare and a conditional return:
//   clrlwi r0,t,24; li r3,8; cmplwi r0,2; bnelr; li r3,7; blr
// Previously this shape SHIPPED WRONG BYTES (`clrlwi; cmplwi; li` — the fire-644 invariant hole),
// then deferred; now it is modeled. A SIGNED narrow operand (extsb path) still defers. (fire 645)
int nii_eq(unsigned char t)  { int a = 8; if (t == 2) { a = 7; } return a; }  // cmplwi; bnelr
int nii_ne(unsigned char t)  { int a = 8; if (t != 2) { a = 7; } return a; }  // cmplwi; beqlr
int nii_lt(unsigned char t)  { int a = 8; if (t < 2)  { a = 7; } return a; }  // cmplwi; bgelr
int nii_gt(unsigned char t)  { int a = 8; if (t > 2)  { a = 7; } return a; }  // cmplwi; blelr
int nii_u16(unsigned short t){ int a = 1; if (t == 9) { a = 3; } return a; }  // clrlwi r0,t,16
