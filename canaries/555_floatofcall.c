// math_ppc.c shape: (float) of a double-returning call rounds the result with
// frsp. Prototype-return tracking tells is_double_value that cos returns double;
// the LR-reload hoist puts the epilogue reload before the frsp.
double cos(double);
float cosf(float arg0) { return (float) cos(arg0); }
