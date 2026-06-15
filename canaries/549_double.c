// `double` type: shares the FPR file with float. A passthrough and a float->double
// widening are register no-ops (the EABI passes/returns doubles in f1). Double
// arithmetic (fadd vs fadds precision), conversions (frsp), and double literals
// are a later stage and defer honestly.
double idd(double x){ return x; }
double widen(float x){ return x; }
double second(double a, double b){ return b; }
