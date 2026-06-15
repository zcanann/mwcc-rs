struct P { int a; int b; };
int memcond(struct P* p,int x,int y){return p->a?x:y;}
