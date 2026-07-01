// A float/double global initializer is a constant EXPRESSION evaluated in double, then
// narrowed to the global's width — not just a single literal. dolphin/MSL headers use
// this (`static float const deg_to_rad = M_PI / 180;` in MSL math.h). Previously only a
// bare literal parsed ("expected Semicolon, found Slash"). Integer literals promote to
// double (mixed `double / int`).
static float const deg_to_rad = 3.14159265358979 / 180;   // double/int, narrowed to float
static float const third      = 1.0f / 3.0f;              // float/float
static double const tau       = 2.0 * 3.14159265358979;   // double*double
float  gf = 1.5f + 0.5f;                                   // computed, referenced below
double gd = 10.0 / 4.0;

float  read_gf(void) { return gf; }   // 2.0f
double read_gd(void) { return gd; }   // 2.5
int    anchor(void)  { return 0; }
