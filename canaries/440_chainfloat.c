struct Q{int x;float f;}; struct P{struct Q* q;};
float chainfloat(struct P* p){return p->q->f;}
