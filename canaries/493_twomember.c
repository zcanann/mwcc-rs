// Two struct members as a multiply operand: the first member loads into a fresh
// virtual the allocator places on a free register (avoiding the base and x), the
// second into the scratch — the register allocator removing another deferral.
struct S { int a; int b; };
int twomember(struct S* p, int x){ return (p->a + p->b) * x; }
