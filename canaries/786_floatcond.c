// A floating-point comparison used as a condition branches off `fcmpo`/`fcmpu`
// (with a `cror` folding equality for `<=`/`>=`), using the same cr0 lt/gt/eq bits
// as an integer compare. A non-leaf that only *compares* its float-register args
// leaves the extab "uses FPU" flag clear (a bare fcmpo doesn't set it — only an FP
// load/store/arith does), so the non-leaf object matches mwcc too.
void sink(void);
void gt(float a, float b)  { if (a > b)  sink(); }
void le(float a, float b)  { if (a <= b) sink(); }
void eq(float a, float b)  { if (a == b) sink(); }
void pos(float a)          { if (a > 0.0f) sink(); }
int  guard(float a, float b) { if (a > b) return 1; return 0; }
