typedef struct { int a, b; } Pr, *PrPtr; int fpr(Pr* p){ return p->a + p->b; }
