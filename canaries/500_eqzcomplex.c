// Compare-against-zero idioms over a both-complex value now lower: the operand
// computes into the scratch (its temporary is a virtual the allocator places),
// then the branchless idiom runs — ((a+b)*(c+d)) == 0 / < 0 / >= 0.
int eqzcomplex(int a, int b, int c, int d){ return ((a + b) * (c + d)) == 0; }
