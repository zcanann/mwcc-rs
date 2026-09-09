// flags: -O4,s
static unsigned a[8192],b[8192],c[80],d[80];
unsigned z[80]={1};
extern void observe4(unsigned*,unsigned*,unsigned*,unsigned*,unsigned);
extern void observe3(unsigned*,unsigned*,unsigned*,unsigned);
void forward(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void reverse(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){r=&c[i];q=&b[i];p=&a[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void middle(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){q=&b[i];p=&a[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void quadruple(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;unsigned*s;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];s=&d[i];*p=i;*q=i+1;*r=i+2;*s=i+3;observe4(p,q,r,s,i);}}
void parameter(unsigned k){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i+k;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void alias(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&a[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void initialized(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&z[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
