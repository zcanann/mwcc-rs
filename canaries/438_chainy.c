struct Q{int x;int y;}; struct P{struct Q* q;int a;};
int chainy(struct P* p){return p->q->y;}
