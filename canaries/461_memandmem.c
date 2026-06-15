struct P{unsigned int a;unsigned char c;int b;};
int memandmem(struct P* p){return p->b&p->a;}
