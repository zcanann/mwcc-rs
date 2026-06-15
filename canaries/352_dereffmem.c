struct P { int a; int* q; float* fq; char* cq; short* sq; };
float dereffmem(struct P* p){return *p->fq;}
