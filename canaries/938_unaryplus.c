// Unary plus is a no-op: it applies only the integer promotions a read already performs, so `+a` is
// exactly `a` and mwcc emits identical code. The parser now accepts it (discarding the `+`) rather
// than failing with "expected an expression, found +". `++` is a distinct token, so pre-increment is
// unaffected.
int plus_scalar(int a)      { return +a; }              // == return a;
int plus_in_expr(int a)     { return +a * 2 + +a; }     // == a*2 + a
int plus_two(int a, int b)  { return +a + +b; }         // == a + b
int neg_of_plus(int a)      { return -+a; }             // == -a
