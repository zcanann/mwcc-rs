struct P { int a; int* q; float* fq; char* cq; short* sq; };
int idxmem(struct P* p,int i){return p->q[i];}
