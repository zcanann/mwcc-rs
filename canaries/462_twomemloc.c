struct P{unsigned int a;unsigned char c;int b;};
int twomemloc(struct P* p){int x=p->b;int y=p->a;return x|y;}
