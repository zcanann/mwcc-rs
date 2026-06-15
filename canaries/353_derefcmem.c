struct P { int a; int* q; float* fq; char* cq; short* sq; };
char derefcmem(struct P* p){return *p->cq;}
