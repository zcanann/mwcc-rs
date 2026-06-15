struct P { int a; int* q; float* fq; char* cq; short* sq; };
void storemem(struct P* p,int v){*p->q=v;}
