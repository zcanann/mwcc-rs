struct P { int a; int* q; float* fq; char* cq; short* sq; };
int derefmem(struct P* p){return *p->q;}
