// `T y; if (c) y = A; else y = B; return y;` — both arms assign the returned local,
// so the body is the select `return (c) ? A : B`, which mwcc compiles identically to
// `if (c) return A; return B` (a branch for parameters, the branchless
// neg/or/srawi/addi for constants).
int sel_params(int x, int a, int b) { int y; if (x)   y = a; else y = b; return y; }
int sel_cond(int x, int a, int b)   { int y; if (x>0) y = a; else y = b; return y; }
int sel_consts(int x)               { int y; if (x)   y = 1; else y = 2; return y; }
