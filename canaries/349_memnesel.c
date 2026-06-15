struct P { int a; int b; };
int memnesel(struct P* p,int x,int y){return p->b!=0?x:y;}
