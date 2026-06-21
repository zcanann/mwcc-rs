// A guard with a negated condition `if (!x) return a; return b;` is NOT the bare
// ternary `(!x)?a:b`: mwcc keeps the *guard value* (a) as the in-place default and
// strips the `!`, compiling `(x) ? b : a` (cmpwi; beq; overwrite a with b when x!=0)
// — whereas a written ternary `(!x)?a:b` keeps the false-arm (b) as default. The
// if/else form `if(!x)return a; else return b;` lowers through the same guard path.
int guard(int x, int a, int b)  { if (!x) return a; return b; }
int ifelse(int x, int a, int b) { if (!x) return a; else return b; }
int chain(int x, int y, int a, int b, int c) { if (x) return a; if (!y) return b; return c; }
