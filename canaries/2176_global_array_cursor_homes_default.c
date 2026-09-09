// flags:
unsigned a[80],b[80],c[80];
extern void observe(unsigned*,unsigned*,unsigned);
void forward(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i;observe(p,q,i);}}
void reverse_declarations(void){unsigned*q;unsigned*p;unsigned i;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i;observe(p,q,i);}}
void reverse_bindings(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){q=&b[i];p=&a[i];*p=i;*q=i;observe(p,q,i);}}
void reverse_uses(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*q=i;*p=i;observe(q,p,i);}}
void hot_second(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i;q[1]=i;q[2]=i;q[3]=i;observe(p,q,i);}}
void bias(unsigned k){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i+k;*q=i;observe(p,q,i);}}
volatile unsigned seed;
void eager(void){unsigned k=seed;unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i+k;*q=i;observe(p,q,i);}}
void return_index(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i;observe(p,q,i);}observe(a,b,i);}
void explicit_cursors(void){unsigned i;unsigned*p=a;unsigned*q=b;for(i=0;i<64;i++){*p=i;*q=i;observe(p,q,i);p++;q++;}}
