// `(cond) ? <arithmetic> : <leaf>` — the mirror of 866: compute a simple arithmetic arm, else
// return a cached leaf. `if (a < 0) return a + 1; return b;` compiles to
// `cmpwi r3,0; bge skip; addi r4,r3,1; skip: mr r3,r4`: forward-branch past the computed arm
// when the condition is false (keeping the leaf in its register), evaluate the true arm INTO
// that register, then `mr`. Restricted to a SIMPLE ARITHMETIC true-arm — a CONSTANT true-arm
// (including `-1`, whose AST is Unary{Negate,1}) is a different shape handled by the
// constant-arm selects, so it is excluded (constant_value folds it out of is_simple_arithmetic_arm).
int arith_add(int a, int b)            { return (a < 0) ? a + 1 : b; }
int arith_mul(int a, int b)            { if (a > 0) return a * 2; return b; }
int arith_neg(int a, int b)            { return (a < 0) ? -a : b; }     // negate of a VARIABLE is computed
int arith_diffvar(int a, int b, int c) { return (a < 0) ? c + 1 : b; }
