struct Q{int x;int y;}; struct P{struct Q* q;int a;};
void chainstore(struct P* p,int v){p->q->x=v;}
