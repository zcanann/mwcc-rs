struct P { int a; int b; char c; short d; float e; int* q; };
unsigned char memuchar(struct P* p){return p->c;}
