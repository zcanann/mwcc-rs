// Double-precision arithmetic uses the opcode-63 forms (fadd/fsub/fmul/fmadd)
// rather than the single fadds/fsubs/fmuls; precision is detected per-node from
// the operand types (a double variable carries width 64).
double dadd(double a, double b){ return a + b; }
double dsub(double a, double b){ return a - b; }
double dmul(double a, double b){ return a * b; }
double dfma(double a, double b, double c){ return a * b + c; }
