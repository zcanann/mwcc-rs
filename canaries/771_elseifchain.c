// An `else if` chain of returning branches is a guard chain: since each branch
// returns, the elses are implied, so `if(w)return a; else if(x)return b; else
// return c;` is the guards [w->a, x->b] with default c — mwcc forward-branches all
// but the last and compiles the last guard with the default as a branchless select.
int two_way(int x, int y, int a, int b, int c)        { if (x) return a; else if (y) return b; else return c; }
int two_way_noelse(int x, int y, int a, int b, int c) { if (x) return a; else if (y) return b; return c; }
int three_way(int w, int x, int y, int a, int b, int c, int d) { if (w) return a; else if (x) return b; else if (y) return c; else return d; }
