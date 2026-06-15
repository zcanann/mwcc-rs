struct Q{int x;float f;}; struct P{struct Q* q;};
int chaincond(struct P* p,int a,int b){return p->q->x?a:b;}
