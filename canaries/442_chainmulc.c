struct Q{int x;float f;}; struct P{struct Q* q;};
int chainmulc(struct P* p){return p->q->x*2;}
