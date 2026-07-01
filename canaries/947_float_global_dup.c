// `gf * gf` / `gf + gf` for a float (or double) GLOBAL reads the same memory on both sides. Unlike a
// register-resident float parameter (a free re-read), a global read is a LOAD, so mwcc loads it ONCE
// and applies the op to that register twice (`lfs f0,gf; fmuls f1,f0,f0`) — not two loads. The float
// codegen's identical-load idiom now covers a global variable (it previously only handled a memory
// deref / member / element).
float  gf;
double gd;
float  fsquare(void) { return gf * gf; }   // lfs f0,gf; fmuls f1,f0,f0
float  fdouble(void) { return gf + gf; }   // lfs f0,gf; fadds f1,f0,f0
double dsquare(void) { return gd * gd; }   // lfd f0,gd; fmul f1,f0,f0
float  fparam(float x) { return x * x; }   // a register value: fmuls f1,f1,f1 (no extra load)
