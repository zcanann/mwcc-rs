// Comparison-to-bool with a non-leaf (member) operand: p->a > x. The member
// loads into a virtual that AVOIDS the destination (a new allocator placement
// hint), leaving the destination free for the > idiom's result-path temporary —
// reproducing mwcc, which keeps that temp in the low destination register.
struct S { int a; };
int gtmember(struct S* p, int x){ return p->a > x; }
