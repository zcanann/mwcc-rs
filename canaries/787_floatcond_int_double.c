// An integer literal in a float comparison is promoted to the comparison's
// precision and loaded from the constant pool, exactly as the written `0.0f`/`0.0`
// (`if (f > 0)` == `if (f > 0.0f)`). For `==`/`!=` the literal canonicalizes to the
// front of the fcmpu, whether written as a float or an int literal.
//
// The extab "uses FPU" flag is keyed on *single*-precision float usage: a non-leaf
// doing only double-precision work (an `lfd` for a double constant, a double `fadd`)
// leaves it clear, so `if (d > 0.0)` and a double store carry no flag while the
// single-precision `if (f > 0.0f)` and a float store do.
//
// (Distinct constants per function: pooled-constant dedup across functions is a
// separate concern tracked elsewhere.)
void sink(void);
void fgt(float a)        { if (a > 0)   sink(); }   // int-lit promoted to 0.0f (lfs)
void feq(float a)        { if (a == 1)  sink(); }   // literal-first fcmpu, 1.0f
void dgt(double a)       { if (a > 2)   sink(); }   // int-lit promoted to 2.0 (lfd), no FPU flag
void dconst(double a)    { if (a > 3.0) sink(); }   // double constant, no FPU flag
void dvar(double a, double b) { if (a <= b) sink(); }
double dstore_g;
void dstore(double a, double b) { dstore_g = a + b; }  // double arith store, no FPU flag
