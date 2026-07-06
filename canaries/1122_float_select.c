// A tail float select `float_cond ? x : y` where both arms are float leaves lowers to a float guard:
// `fcmpo cr0,f1,fX; b<cc>lr; fmr f1,fX; blr` (the arm already in the result register f1 returns via
// the branch; the other is the fall-through `fmr`). For `<=` / `>=` the branch must be FALSE on
// unordered (NaN) operands, so mwcc ORs the strict bit into the eq bit (`cror eq, lt|gt, eq`) and
// branches on eq — a direct `ble`/`bge` would wrongly branch when unordered. (fire 596)
double dmin(double a, double b)     { return a <  b ? a : b; }   // fcmpo; bltlr; fmr f1,f2
double dmax(double a, double b)     { return a >  b ? a : b; }   // fcmpo; bgtlr; fmr
double dle(double a, double b)      { return a <= b ? a : b; }   // fcmpo; cror eq,lt,eq; beqlr; fmr
double dge(double a, double b)      { return a >= b ? a : b; }   // fcmpo; cror eq,gt,eq; beqlr; fmr
double deq(double a, double b)      { return a == b ? a : b; }   // fcmpo; beqlr; fmr
double dne(double a, double b)      { return a != b ? a : b; }   // fcmpo; bnelr; fmr
double dle_swapped(double a, double b) { return a <= b ? b : a; } // inverted: fcmpo; cror; bnelr; fmr
float  fmin(float a, float b)       { return a <  b ? a : b; }   // single-precision leaves

// The fabs family `cond ? -x : x` (a leaf and its negation): the plain leaf returns via the branch,
// the negated arm is an `fneg` tail. The CONSTANT comparison operand loads at the comparison WIDTH —
// `lfd` for a double (an int literal `0` promotes to double), `lfs` for single — not always `lfs`.
double dfabs(double a)    { return a < 0    ? -a : a; }   // lfd f0,0; fcmpo; bgelr; fneg f1,f1
double dfabs_ge(double a) { return a >= 0   ?  a : -a; }  // lfd; fcmpo; cror eq,gt,eq; beqlr; fneg
double dfabs_lit(double a){ return a < 0.0  ? -a : a; }   // double float-literal also lfd
float  ffabs(float a)     { return a < 0.0f ? -a : a; }   // lfs; fcmpo; bgelr; fneg
double dclamp_lo(double a, double b) { return a < 0.0 ? a : b; }  // double-width constant: lfd, not lfs
