// Mirror of 526: the non-leaf operand on the LEFT (p->a != x). Still evaluated
// into the scratch; the leaf right keeps its home register.
struct S { int a; };
int nememberl(struct S* p, int x){ return p->a != x; }
