// Mixed `int OP float` arithmetic promotes the integer operand to float before the op. mwcc emits the
// magic-constant int->float idiom into the SCRATCH fp register (bias in f2, avoiding the live float
// operand in f1), then the float op: `int a + float b` -> [convert a into f0]; fadds f1,f0,f1. Works
// for +,-,*,/ in either operand order (the converted integer keeps its source position); int->double
// promotion uses fsub/fadd. A narrow (char/short) source is a separate version-divergent idiom (it
// reschedules around extsb/extsh) and still defers — never wrong bytes.
float  iaf(int a, float b)    { return a + b; }   // convert a->f0; fadds f1,f0,f1
float  fai(float a, int b)    { return a + b; }   // convert b->f0; fadds f1,f1,f0
float  isf(int a, float b)    { return a - b; }   // fsubs f1,f0,f1
float  fsi(float a, int b)    { return a - b; }   // fsubs f1,f1,f0
float  imf(int a, float b)    { return a * b; }   // fmuls f1,f0,f1
float  idf(int a, float b)    { return a / b; }   // fdivs f1,f0,f1
double iad(int a, double b)   { return a + b; }   // int->double; fadd f1,f0,f1
double dmi(double a, int b)   { return a * b; }   // fmul f1,f1,f0
