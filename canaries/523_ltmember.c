// Mirror of 520: a non-leaf RIGHT operand, x < p->a. The `<` idiom keeps its
// right operand (used in both the xor and the and), so mwcc holds it in a
// register; computing the member into a virtual that AVOIDS the destination
// leaves the destination free for the idiom's result-path temporary.
struct S { int a; };
int ltmember(struct S* p, int x){ return x < p->a; }
