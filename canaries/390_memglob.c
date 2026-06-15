int g; struct P{int a;};
int memglob(struct P* p){return p->a+g;}
