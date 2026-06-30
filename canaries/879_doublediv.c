// Double-precision division uses `fdiv` (opcode 63), NOT the single-precision `fdivs` (opcode 59).
// The float combiner only had a single-precision Divide arm, so a `double` divide was computed at
// SINGLE precision — a miscompile (wrong low mantissa bits). A new FloatDivideDouble instruction
// and a `Divide if double` arm fix it; float division still emits `fdivs`. (Double add/sub/mul
// already picked the double-precision form.)
double divd(double a, double b)            { return a / b; }
double divhalf(double a)                   { return a / 2.0; }
double divchain(double a, double b, double c) { return a / b / c; }
float  divf(float a, float b)              { return a / b; }
