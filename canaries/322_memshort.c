struct P { int a; int b; char c; short d; float e; int* q; };
short memshort(struct P* p){return p->d;}
