int g; struct P{int a;};
int globmem(struct P* p){return g+p->a;}
