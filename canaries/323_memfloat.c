struct P { int a; int b; char c; short d; float e; int* q; };
float memfloat(struct P* p){return p->e;}
