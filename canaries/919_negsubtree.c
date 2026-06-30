// (1) FOLD: mwcc folds a unary minus into a subtract — `-a + b` -> `b - a` (subf r3,r3,r4),
// `a + -b` -> `a - b`, `-(a*b) + c` -> `c - a*b`. We rewrite an Add with a Negate operand to the
// equivalent Subtract (the subf operand order matches). These were DIFFs (ours emitted neg + add).
// (2) DEFER: a SUBTRACT whose BOTH operands are computed binary expressions ((a-b)-(c-d), a*b-c*d)
// evaluates the sub-trees in an order our straight-line path doesn't match (subtract isn't
// commutative, no overlap idiom) and defers; a leaf/constant operand keeps the byte-exact shape.
int neg_l(int a, int b)          { return -a + b; }      // subf r3,r3,r4 (b - a)
int neg_r(int a, int b)          { return a + -b; }      // subf r3,r4,r3 (a - b)
int neg_mul(int a, int b, int c) { return -(a * b) + c; }// c - a*b
int sub_chain(int a, int b, int c){ return a - b - c; }  // (a-b)-c, leaf right
int sub_nest(int a, int b, int c) { return a - (b - c); }// leaf left
