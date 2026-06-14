struct P { int a; int b; char c; short d; float e; };
int varmem(struct P* p,int x){return x+p->a;}
