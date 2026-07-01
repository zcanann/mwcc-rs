// A `(double)` cast of an already-`double` value is a semantic no-op — mwcc emits
// NOTHING for it (no `frsp`, no `fmr`). The value simply stays in its register.
// Previously we treated any float-typed cast as a narrowing and emitted a spurious
// `frsp` (the `w_atan2.c`/`w_pow.c` real-file DIFF: `return (f64)__ieee754_atan2()`).
//   return (double)dbl_call();  -> bl call; blr            (no frsp on the double result)
//   *p = (double)x;             -> stfd f1,0(r3)           (store the leaf directly, no fmr)
//   gd = (double)x;             -> stfd f1,gd              (store to a double global directly)
// A real `(float)` narrowing of a double still rounds with `frsp` (unchanged).
extern double dcall(void);
double gd;

double ret_cast_call(double a, double b) { return (double)dcall(); }   // no frsp
double ret_cast_leaf(double x)           { return (double)x; }         // no-op
float  ret_narrow(double x)              { return (float)x; }          // frsp (narrowing)
void   store_deref(double *p, double x)  { *p = (double)x; }           // stfd leaf, no fmr
void   store_global(double x)            { gd = (double)x; }           // stfd to global, no fmr
