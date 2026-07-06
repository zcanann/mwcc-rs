// When a guard's early-return VALUE already occupies the result register (`if(a>0) return a;` with a
// in r3) and a store continuation follows, mwcc collapses the guard to a single conditional
// branch-to-lr (`bgtlr`) — NOT a forward branch over a no-op value move — then materializes the store
// value and the return: `if(a>0) return a; *p=5; return 0;` -> `cmpwi r3,0; bgtlr; li r0,5; li r3,0;
// stw r0,0(r4); blr`. A MATERIALIZED store value (constant, or a two-leaf `a+1`) lands in r0 with the
// return scheduled between; the guard-value-NOT-in-r3 form keeps its forward branch (canary coverage
// unchanged). (fire 608; register-leaf store values like `*p=a` are a separate emission — follow-up.)
int gr_const(int a, int* p)      { if (a > 0) return a; *p = 5;   return 0; }  // bgtlr; li r0,5; li r3,0; stw r0,0(r4)
int gr_twoleaf(int a, int* p)    { if (a > 0) return a; *p = a+1; return 0; }  // bgtlr; addi r0,r3,1; li r3,0; stw r0,0(r4)
int gr_indexed(int a, int* p)    { if (a > 0) return a; p[2] = 5; return 0; }  // bgtlr; li r0,5; li r3,0; stw r0,8(r4)
int gr_notinr3(int a, int* p)    { if (a > 0) return 5; *p = 1;   return 0; }  // keeps forward branch (value 5 not in r3)
