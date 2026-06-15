struct P{int a;};
int vtmemmut(struct P* p){int y=p->a;y=y+1;return y;}
