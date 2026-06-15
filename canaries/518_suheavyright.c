// Sethi-Ullman evaluation order: mwcc computes the operand needing more registers
// first. The heavier subtree (right) is evaluated into the scratch, the lighter
// (a+b) into the destination: (a+b)*((c+d)*(e+g)). Byte-exact via register_need.
int suheavyright(int a, int b, int c, int d, int e, int g){ return (a + b) * ((c + d) * (e + g)); }
