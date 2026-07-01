// Two output pointers, each receiving a call result. Both pointers must survive their
// calls, so mwcc parks them in callee-saved registers -- the pointer arriving in the
// HIGHER incoming register (r4) in r31, the lower (r3) in r30, positionally (independent
// of which store runs first) -- runs each call, stores the result, then reloads LR before
// both GPRs (`lwz r0,20; lwz r31,12; lwz r30,8; mtlr`). Covers *p, p[const], p->member.
// Previously deferred (single-pointer only), and the p->member form MISCOMPILED (stored
// through the raw, clobbered argument register).
struct S { int a; int x; };
extern int g(void);
extern int h(void);

void two_deref(int *a, int *b)              { *a = g();  *b = h(); }   // r30<-a, r31<-b
void two_deref_swapped(int *a, int *b)      { *b = g();  *a = h(); }   // store order swapped
void two_index(int *a, int *b)              { a[1] = g(); b[2] = h(); }
void two_member(struct S *a, struct S *b)   { a->x = g(); b->x = h(); }
