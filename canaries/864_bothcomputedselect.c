// `(cond) ? <expr1> : <expr2>` where BOTH arms are computed (neither a leaf nor a constant)
// compiles to a branch select: stage the FALSE arm in r0, forward-branch past the true arm when
// the condition is false (keeping the false arm), evaluate the true arm into r0, then
// `mr dest, r0`. So `(a<0) ? a+1 : a-1` is
// `cmpwi r3,0; addi r0,r3,-1; bge skip; addi r0,r3,1; skip: mr r3,r0`. Appears as
// `return cond ? f(x) : g(x)` and as a guard `if (cond) return e1; return e2;` with computed e1/e2.
int both_a(int a)           { return (a < 0) ? a + 1 : a - 1; }
int both_b(int a)           { return (a > 0) ? a * 2 : a + 1; }
int both_guard(int a)       { if (a < 0) return -a; return a + 1; }
int both_vars(int a, int b) { return (a < 0) ? a + 1 : b - 1; }
int both_eq(int a)          { return (a == 0) ? a + 5 : a - 5; }
