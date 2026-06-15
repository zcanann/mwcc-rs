struct P { int a; int* q; float* fq; char* cq; short* sq; };
int idxmemc(struct P* p){return p->q[2];}
