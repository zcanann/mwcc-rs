// An operand that appears with opposite signs cancels: `(X + Y) - Y` is `X`, `(X + Y) - X` is
// `Y`, `(X - Y) + Y` is `X`. mwcc folds straight to the survivor (`(a+b)-b` is a bare `blr`;
// `(a+5)-a` is `li r3,5`). ours had computed the inner op then undone it. The cancelling
// operand must be a side-effect-free LEAF (variable/constant) so dropping its evaluation is
// safe — a call operand `(a + h()) - h()` does NOT fold (the two calls are distinct).
int cancel_r(int a, int b)        { return (a + b) - b; }       // -> a
int cancel_l(int a, int b)        { return (a + b) - a; }       // -> b
int cancel_sub(int a, int b)      { return (a - b) + b; }       // -> a
int cancel_const(int a)           { return (a + 5) - a; }       // -> 5 (li r3,5)
int cancel_commuted(int a, int b) { return (b + a) - b; }       // -> a
int cancel_survivor(int a, int b) { return (a * 2 + b) - b; }   // -> a*2 (survivor computed)
