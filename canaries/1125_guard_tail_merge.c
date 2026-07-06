// When a value-tracking guard's VALUE lives in the result register (`if(a>0) return a;` with a in r3)
// and the fall-through returns a COMPUTED value, mwcc merges both return paths through r0: the tail
// computes into r0, a forward branch on the INVERTED guard skips the guard-value load, and one
// `mr r3,r0` merges — `cmpwi; addi r0,b,1; ble skip; mr r0,a; skip: mr r3,r0; blr`. (This shape
// previously MISCOMPILED via the fold `mr r3,a` self-move, then deferred; fire 600 reproduces it.)
int tm_gt(int a, int b)       { if (a > 0)  return a; b = b + 1; return b; }   // ble skip; slot addi r0
int tm_lt(int a, int b)       { if (a < 0)  return a; b = b - 1; return b; }   // bge skip; addi r0,-1
int tm_bool(int a, int b)     { if (a)      return a; b = b * 2; return b; }   // beq skip; slwi r0
int tm_ge(int a, int b)       { if (a >= 0) return a; b = b + 5; return b; }
int tm_local(int a, int b)    { int c; if (a > 0) return a; c = b + 1; return c; }  // reassign a local
