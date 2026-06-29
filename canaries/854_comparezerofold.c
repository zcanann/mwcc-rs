// Comparing a comparison against zero collapses to a single comparison: `(a < b) == 0` is the
// NEGATED comparison `a >= b`, and `(a < b) != 0` is just `a < b`. ours computed the inner
// comparison to a 0/1 value and then tested THAT against zero (two idioms); mwcc folds it (and
// `!(a < b)` already did). Now the `== 0` / `!= 0` arms recognize a comparison operand.
int ne_lt(int a, int b)  { return (a < b) == 0; }   // a >= b
int ne_le(int a, int b)  { return (a <= b) == 0; }  // a > b
int ne_eqz(int a)        { return (a == 0) == 0; }  // a != 0
int ne_eq(int a, int b)  { return (a == b) == 0; }  // a != b
int ne_ne(int a, int b)  { return (a != b) == 0; }  // a == b
int id_lt(int a, int b)  { return (a < b) != 0; }   // a < b
int id_eq(int a, int b)  { return (a == b) != 0; }  // a == b
