struct Q{int x;float f;}; struct P{struct Q* q;};
int chainaddv(struct P* p,int i){return p->q->x+i;}
