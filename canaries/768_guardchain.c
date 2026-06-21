// A chain of early-return guards: mwcc forward-branches all but the last, then
// compiles the final guard together with the fall-through return as a single
// branchless select `(cond) ? value : final` — the same form as a lone guard,
// not a third branch.
int two_guards(int x, int y)               { if (x) return y; if (y) return x; return 0; }
int three_guards(int a, int b, int c, int d) { if (a) return b; if (b) return c; if (c) return d; return a; }
int last_select(int x, int y)              { if (y) return x; return 0; }
