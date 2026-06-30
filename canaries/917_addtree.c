// mwcc REASSOCIATES integer add-CHAINS of 3+ additions (`a+b+c+d` -> `a+((b+c)+d)`, computing b+c
// first) and evaluates a nested-add operand in its own order; our register allocator emits a valid
// but non-matching sequence, so a reassociated add-tree DEFERS (never wrong bytes) until the #20
// keystone allocator matches it. The simple shapes below are byte-exact and stay: a left-assoc chain
// up to (a+b)+c, an add with a non-add operand (a+b*c, a*b+c*d), an add-chain CONSUMED by a non-add
// (the a+b in (a+b)*c+a is a 1-add chain), and a simple sub-add nested in a product.
int chain2(int a, int b, int c)        { return a + b + c; }      // (a+b)+c
int addmul(int a, int b, int c)        { return a + b * c; }      // a+(b*c)
int twomul(int a, int b, int c, int d) { return a * b + c * d; }  // (a*b)+(c*d)
int consumed(int a, int b, int c)      { return (a + b) * c + a; }// a+b consumed by *c -> 1-add chain
int prod(int a, int b, int c, int d)   { return (a + b + c) * d; }// simple 3-add chain inside a product
