struct Q{int x;int y;}; struct P{struct Q* q;int a;};
int chainx(struct P* p){return p->q->x;}
