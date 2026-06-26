// A float/double global read as a STORE value — `gf = gg;` — was rejected ("unknown
// variable") because the float store-value path only handled register leaves (parameters),
// not globals. A float global is not in `locations`, so route it to the general float
// evaluator, which loads it: `lfs f0,gg; stfs f0,gf` (the float return path already did
// this for `return gf`). Multiple such stores reschedule the load (mwcc loads the global
// once and reuses it) and defer; a single store needs no scheduling.
float gf, gg;
double gd, ge;
void copy_float(void)   { gf = gg; }   // lfs f0,gg; stfs f0,gf
void copy_double(void)  { gd = ge; }   // lfd f0,ge; stfd f0,gd
