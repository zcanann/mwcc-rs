// A variable shift by a register amount shifts straight from the value's home
// register into the destination (`srw r0,r3,r4`); ours moved the value into the
// destination first (`mr r0,r3; srw r0,r0,r4`), a redundant instruction. A leaf stays
// in place; a sub-expression still evaluates into the destination.
unsigned ur; int sr;
void shr_u(unsigned a, int n) { ur = a >> n; }   // srw r0,r3,r4
void shr_s(int a, int n)      { sr = a >> n; }    // sraw
void shl_u(unsigned a, int n) { ur = a << n; }    // slw
int  ret_shr(int a, int n)    { return a >> n; }
void shr_expr(unsigned a, int n) { ur = (a + 1) >> n; }   // sub-expression into r0
