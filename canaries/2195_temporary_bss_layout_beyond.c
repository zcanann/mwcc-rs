// flags:
static unsigned a[4096],b[4096],c[80];
extern void observe(unsigned*,unsigned*,unsigned*,unsigned);
void walk(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe(p,q,r,i);}}
