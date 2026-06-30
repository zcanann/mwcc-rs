// int<->float conversions (#22). mwcc's int->float: bias the int (xoris its sign bit), assemble the
// double 0x43300000_<biased int> on the frame, subtract the magic bias double (fsubs/fsub); its
// float->int: fctiwz then bounce through the frame (stfd/lwz). The basic int/unsigned source widths
// are byte-exact. A NARROW (char/short) source first widens to int (extsb/extsh) and mwcc reschedules
// the magic-constant idiom around that extra instruction — not modeled, so char/short->float DEFERS
// (an honest defer, never wrong bytes) rather than emitting the int-width idiom unextended.
float    i2f(int x)              { return (float)x; }   // xoris;...;fsubs
double   i2d(int x)              { return (double)x; }  // xoris;...;fsub
float    u2f(unsigned x)         { return (float)x; }   // no xoris (unsigned bias)
int      f2i(float x)            { return (int)x; }     // fctiwz;stfd;lwz
int      d2i(double x)           { return (int)x; }     // fctiwz;stfd;lwz
double   f2d(float x)            { return x; }           // (no-op widen)
float    d2f(double x)           { return x; }           // frsp
void     cstore(int x, float *p) { *p = (float)x; }     // conversion into a store
int      fcmp(float a, float b)  { return a < b; }      // float compare -> int
