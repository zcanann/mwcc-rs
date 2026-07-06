// The value-tracking guard folds (`if(cond) return V; <reassign>; return tail;`) compute the tail
// INTO the result register. When the guard's VALUE lives in that register (`if(a>0) return a;` with a
// in r3), the tail clobbers it and the fold's `mr r3,a` is a dead self-move — that MISCOMPILED (the
// a>0 path returned the tail value); it now defers (fire 599). The folds still apply, byte-exact, when
// the guard value is a constant or a variable held OUTSIDE the result register:
int guard_other_param(int a, int b, int c) { if (a > 0) return c; b = b + 1; return b; }  // c in r5 -> mr r3,c
int guard_constant(int a, int b)           { if (a > 0) return 1; b = b + 1; return b; }  // li r3,1 fold
