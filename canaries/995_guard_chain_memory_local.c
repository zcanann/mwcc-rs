// A GUARD CHAIN over the r31 memory-local (the raise() dispatch prologue): every guard
// compares the STAGED r0 copy (valid across the chain -- no call intervenes), each
// constant early return branches to the shared epilogue, and only the FIRST compare
// carries `mr r31,r0` in its latency slot:
//   lwz r0,gi; cmpwi r0,0; mr r31,r0; bne G2; li r3,-1; b EPI;
//   G2: cmpwi r0,1; bne CONT; li r3,0; b EPI;
//   CONT: bl; mr r3,r31; EPI: lwz r0,20; lwz r31,12; mtlr; addi; blr
int gi;
extern void g(void);

int two_guards(void)   { int t = gi; if (!t) return -1; if (t == 1) return 0; g(); return t; }
int three_guards(void) { int t = gi; if (!t) return -1; if (t == 1) return 0; if (t > 9) return 5; g(); return t; }
