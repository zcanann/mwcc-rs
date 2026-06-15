struct Q{int x;float f;}; struct P{struct Q* q;};
int chaineq0(struct P* p){return p->q->x==0;}
