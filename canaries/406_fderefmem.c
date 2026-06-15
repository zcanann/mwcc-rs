struct P{float e;};
float fderefmem(float* p,struct P* q){return *p+q->e;}
