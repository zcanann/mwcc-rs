// Callee-saved for a LOCAL that is a call result: a value produced by a call and
// returned after further calls is preserved in r31 across them —
// `int z = g(); h(); return z;` -> save r31, `bl g; mr r31,r3; bl h; mr r3,r31`,
// restore r31. The return may post-process z with constants (`z+1`, `z*2`); a
// parameter or global in it (a second live / rescheduled value) still defers.
extern int csr_make(void);
extern void csr_step(void);
extern void csr_more(void);
int csr_one(void)    { int z = csr_make(); csr_step(); return z; }
int csr_two(void)    { int z = csr_make(); csr_step(); csr_more(); return z; }
int csr_plus(void)   { int z = csr_make(); csr_step(); return z + 1; }
int csr_scale(void)  { int z = csr_make(); csr_step(); return z * 2; }
// Two call-result locals: each preserved in its own callee-saved register
// (a->r30 first, b->r31 second), combined in a single low-latency op in the
// return (`a + b`); `a * b` and three-plus locals still defer.
extern int csr_g1(void);
extern int csr_g2(void);
int csr_pair(void) { int a = csr_g1(); int b = csr_g2(); csr_step(); return a + b; }
// A single producing call may forward the function's parameters in their natural
// register positions (`g1(x)`, `g1(x, y)`) — they are already in place, so the
// sequence matches the argument-free shape. A constant/reordered argument, or a
// two-local producing call with arguments, still defers.
extern int csr_use1(int);
extern int csr_use2(int, int);
int csr_fwd1(int x)        { int z = csr_use1(x); csr_step(); return z; }
int csr_fwd2(int x, int y) { int z = csr_use2(x, y); csr_step(); return z + 1; }
