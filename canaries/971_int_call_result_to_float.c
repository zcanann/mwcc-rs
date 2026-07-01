// A call whose result is used as a float/double but which returns INT — an implicitly
// declared callee (no prototype, defaults to int), the libm `w_*` wrappers:
// `double acos(double x){ return __ieee754_acos(x); }`. The int result (r3) is converted
// to the CONTEXT precision with the magic-bias sequence, reusing the non-leaf call
// prologue's frame (no second stwu). mwcc schedules the call-result conversion
// value-store-FIRST (the call->xoris->stw value chain is the critical path):
//   bl g; xoris r3,r3,0x8000; lis r0,0x4330; stw r3,12(r1); lfd f1,bias; stw r0,8(r1);
//   lfd f0,8(r1); fsub f1,f0,f1     (fsub for double, fsubs for float)
double acos(double x)  { return __ieee754_acos(x); }   // -> double (fsub)
float  acosf(float x)  { return __ieee754_acosf(x); }  // -> float  (fsubs)
double from_int_arg(int n) { return __compute(n); }    // int arg, implicit int result
