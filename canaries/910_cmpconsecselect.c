// `cmp ? c1 : c2` with consecutive constant arms and c1 > c2 (INCREASING) folds to the bare
// comparison (0/1, computed exactly as `return a REL b`) plus the smaller constant: `<cmp>; addi
// d,d,min`. Covers all six comparison operators, and the equivalent if-else-assign
// (`if(cmp) b=c1; else b=c2; return b;` — the initializer, if any, is dead). The DECREASING case
// (c1 < c2) is mwcc's `-(cmp)+c1` negated-mask (subfc/subfe) idiom, still deferred (not diffed).
int sel_gt(int a)           { return a > 3 ? 2 : 1; }    // (a>3)+1
int sel_ge(int a)           { return a >= 3 ? 11 : 10; }
int sel_lt(int a)           { return a < 3 ? 8 : 7; }
int sel_le(int a)           { return a <= 0 ? 2 : 1; }
int sel_eq(int a)           { return a == 3 ? 2 : 1; }
int sel_ne(int a)           { return a != 0 ? 5 : 4; }
int sel_two_vars(int a, int b) { return a > b ? 2 : 1; }
int if_else(int a)          { int b; if (a > 3) b = 2; else b = 1; return b; }
int if_else_init(int a)     { int b = 0; if (a > 3) b = 2; else b = 1; return b; }
