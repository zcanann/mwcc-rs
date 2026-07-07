// A computed local KEPT in the result register, stored, then returned:
// `int t = <single-op value>; *p = t; [*q = t; …] return t;`. Because t is returned it lives in r3;
// mwcc computes it ONCE there, stores from r3, and returns from r3 — it does not recompute the value
// for the store and the return separately. This is the register-kept slice of the "value tracking with
// stores or calls" cluster: the general value_tracking pass INLINES a local's value (which would emit
// the computation twice), so this shape was deferred. The store targets are bare pointer derefs of
// parameters, whose address never touches r3, so t survives every store to the return.
//   int t=a+1; *p=t; return t;  ->  addi r3,r3,1; stw r3,0(r4); blr
// A non-bare return (`return t+1`), an intervening call, or a non-pointer-deref store target defers.
// (fire 629 — value-tracking #20/#9 seam)
int clsr_add(int a, int* p)          { int t = a + 1; *p = t; return t; }        // addi r3,r3,1; stw r3,0(r4)
int clsr_mul(int a, int b, int* p)   { int t = a * b; *p = t; return t; }        // mullw r3,r3,r4; stw r3,0(r5)
int clsr_addv(int a, int b, int* p)  { int t = a + b; *p = t; return t; }        // add r3,r3,r4; stw r3,0(r5)
int clsr_two(int a, int* p, int* q)  { int t = a + 1; *p = t; *q = t; return t; }// addi; stw r3,0(r4); stw r3,0(r5)
int clsr_char(int a, char* p)        { int t = a + 1; *p = t; return t; }        // addi r3,r3,1; stb r3,0(r4)
