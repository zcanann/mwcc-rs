// A call's result stored through a pointer PARAMETER that must survive the call: mwcc
// saves the pointer in a callee-saved register (r31), runs the call, then stores the
// result through r31 -- `mr r31,r3; bl g; stw r3,off(r31)` -- with the store-sink
// epilogue reloading LR before r31. Covers `*p`, `p[const]`, and `p->member` targets.
// (Previously deferred for *p/p[i], and MISCOMPILED for p->member.)
struct S { int a; int x; };
extern int g(void);
extern int h(int);

void store_deref(int *p)        { *p = g(); }        // stw r3,0(r31)
void store_index(int *p)        { p[2] = g(); }       // stw r3,8(r31)
void store_arg(int *p, int x)   { *p = h(x); }        // mr r31,r3; mr r3,r4; bl h; stw
void store_member(struct S *p)  { p->x = g(); }       // stw r3,4(r31)
void store_char(char *p)        { *p = g(); }         // stb r3,0(r31)
