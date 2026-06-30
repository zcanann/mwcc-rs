// Any constant subexpression folds to its value (`li r3,N`), as mwcc does: the comparison and
// logical operators (`5>3`, `5&&0`, `0||7`), logical-not (`!0`), and compound constant expressions
// — not just the arithmetic/bitwise/shift that already folded. Folded in the expression parser
// (binary on two integer literals; unary on one) via fold_constant_expression; runtime operands are
// unchanged.
int cmp_gt(void)    { return 5 > 3; }              // 1
int cmp_lt(void)    { return 5 < 3; }              // 0
int cmp_ne(void)    { return 5 != 3; }             // 1
int cmp_ge(void)    { return 5 >= 5; }             // 1
int log_and(void)   { return 5 && 0; }             // 0
int log_or(void)    { return 0 || 7; }             // 1
int log_not0(void)  { return !0; }                 // 1
int log_not5(void)  { return !5; }                 // 0
int compound(void)  { return (2 + 3) * 4 - 1; }    // 19
int cmp_sum(void)   { return (10 > 5) + (3 < 1); } // 1
int neg_mul(void)   { return -(5 * 2); }           // -10
