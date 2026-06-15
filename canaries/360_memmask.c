struct P { int a; int b; short d; float e; float f; };
int memmask(struct P* p){return p->a&0xff;}
