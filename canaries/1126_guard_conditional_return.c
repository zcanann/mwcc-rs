// A guard whose VALUE already occupies the result register (`if(a>0) return a;` with a in r3),
// followed by a two-parameter reassignment continuation, returns via a single conditional branch-to-lr
// (`bgtlr`) — NOT a forward branch over a `blr` whose value move would be a no-op. So
// `if(a>0) return a; c=b+c; return c;` is `cmpwi; bgtlr; add r3,b,c; blr`. (fire 602; the two-param
// tail keeps its branch form here, distinct from the single-param tail-merge of canary 1125.)
int cr_add(int a, int b, int c)  { if (a > 0) return a; c = b + c; return c; }  // bgtlr; add r3,r4,r5
int cr_lt(int a, int b, int c)   { if (a < 0) return a; c = b * c; return c; }  // bltlr; mullw
int cr_bool(int a, int b, int c) { if (a)     return a; c = b - c; return c; }  // bnelr; subf
int cr_ge(int a, int b, int c)   { if (a >= 0) return a; c = b + c; return c; } // bgelr; add
