struct P { int a; int b; float e; int* q; };
int f(float);
int fmemarg(struct P* p){return f(p->e);}
