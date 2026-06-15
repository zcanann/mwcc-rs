// Scheduling on virtual registers (before allocation) lets the multiply hoist
// across the operand boundary without a false register dependency:
// (a+b)*((c*d)+1) -> mullw c*d; add a+b; addi +1; mullw.
int schedmulhoist2(int a, int b, int c, int d){ return (a + b) * ((c * d) + 1); }
