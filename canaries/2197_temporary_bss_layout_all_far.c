// flags:
static unsigned a[8192],b[8192],c[80];
extern void observe(unsigned*,unsigned*,unsigned*,unsigned);
void walk(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe(p,q,r,i);}}
