// marioparty4 MSL_C math_ppc.c (reduced to the math prototypes): the float
// wrappers around the double math routines, whole-object byte-exact.
double acos(double);
double sin(double);
double cos(double);
float acosf(float arg0) { return (float) acos(arg0); }
float sinf(float arg0) { return (float) sin(arg0); }
float cosf(float arg0) { return (float) cos(arg0); }
