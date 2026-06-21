// First callee-saved case for a LOCAL that is a call result: a value produced by a
// call and returned after further calls is preserved in r31 across them —
// `int z = g(); h(); return z;` -> save r31, `bl g; mr r31,r3; bl h; mr r3,r31`,
// restore r31. (A local initializer's call is also numbered ahead of the body's, so
// g precedes h in the symbol table.) Argument-bearing calls defer for now.
extern int csr_make(void);
extern void csr_step(void);
extern void csr_more(void);
int csr_one(void)  { int z = csr_make(); csr_step(); return z; }
int csr_two(void)  { int z = csr_make(); csr_step(); csr_more(); return z; }
