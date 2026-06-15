struct P { int a; int b; float e; int* q; };
void stcompmem(struct P* p,int v){*p->q=v+1;}
