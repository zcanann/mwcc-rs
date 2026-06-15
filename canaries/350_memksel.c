struct P { int a; int b; };
int memksel(struct P* p,int x,int y,int k){return p->a==k?x:y;}
