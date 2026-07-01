// `__fabs(x)` is an mwcc intrinsic that lowers to the single `fabs` instruction, NOT
// an out-of-line call — so the function stays a LEAF (no stack frame). The pikmin
// libm real-file DIFF: `f64 fabs(f64 x) { return __fabs(x); }` -> `fabs f1,f1; blr`
// (we previously emitted `bl __fabs` inside a full frame).
//   return __fabs(x);        -> fabs f1,f1; blr        (leaf: no stwu/mflr)
//   return __fabs(x + y);    -> fadd f0,..; fabs f1,f0  (operand through the scratch)
// A real call in the argument still makes the function non-leaf (frame preserved).
extern double __fabs(double);

double abs_leaf(double x)            { return __fabs(x); }
double abs_float_leaf(float x)       { return __fabs(x); }
double abs_sum(double x, double y)   { return __fabs(x + y); }
double gd;
double abs_global(void)              { return __fabs(gd); }
