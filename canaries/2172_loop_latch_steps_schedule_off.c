// flags: -schedule off
extern void observe(unsigned*,unsigned*,unsigned); unsigned a[64],b[64];
void cursors(unsigned*p,unsigned*q){unsigned i;for(i=0;i<64;i++){*p=i;*q=i;observe(p,q,i);p++;q++;}}
void signed_index(unsigned*p,unsigned*q){int i;for(i=0;i<64;i++){*p=i;*q=i;observe(p,q,i);p++;q++;}}
void descending(unsigned*p,unsigned*q){unsigned i;for(i=64;i!=0;i--){*p=i;*q=i;observe(p,q,i);p++;q++;}}
void stride(unsigned*p,unsigned*q){unsigned i;for(i=0;i<64;i+=2){*p=i;*q=i;observe(p,q,i);p+=3;q+=5;}}
void dynamic(unsigned*p,unsigned*q,unsigned n){unsigned i;for(i=0;i<n;i++){*p=i;*q=i;observe(p,q,i);p++;q++;}}
void pointer_test(unsigned*p,unsigned*q,unsigned*end){while(p!=end){*q=*p;observe(p,q,0);p++;q++;}}
void global_indexed(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i;observe(p,q,i);}}
void single(unsigned*p){unsigned i;for(i=0;i<64;i++){*p=i;observe(p,p,i);p++;}}
extern void observe3(unsigned*,unsigned*,unsigned*,unsigned);
void three(unsigned*p,unsigned*q,unsigned*r){unsigned i;for(i=0;i<64;i++){*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);p++;q+=2;r+=3;}}
void equals(unsigned*p,unsigned*q){unsigned i;for(i=0;i!=64;i++){*p=i;*q=i;observe(p,q,i);p++;q++;}}
