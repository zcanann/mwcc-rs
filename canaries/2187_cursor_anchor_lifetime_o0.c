// flags: -O0
unsigned a[80],b[80],c[80]; static unsigned sa[80],sb[80],sc[80];
unsigned fa[8192],fb[8192],fc[80];
extern void observe(unsigned*,unsigned*,unsigned);
extern void observe3(unsigned*,unsigned*,unsigned*,unsigned);
extern void flush(unsigned*,unsigned);
void public_setup2(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i+1;observe(p,q,i);}}
void public_tail2(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i+1;observe(p,q,i);}flush(a,64);}
void public_setup3(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void public_tail3(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}flush(a,64);}
void public_before2(void){unsigned i;unsigned*p;unsigned*q;observe(a,b,999);for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i+1;observe(p,q,i);}}
void public_body2(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&a[i];q=&b[i];*p=i;*q=i+1;a[0]=i+7;observe(p,q,i);}}
void local_setup2(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&sa[i];q=&sb[i];*p=i;*q=i+1;observe(p,q,i);}}
void local_tail2(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&sa[i];q=&sb[i];*p=i;*q=i+1;observe(p,q,i);}flush(sa,64);}
void local_setup3(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&sa[i];q=&sb[i];r=&sc[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void local_tail3(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&sa[i];q=&sb[i];r=&sc[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}flush(sa,64);}
void local_before2(void){unsigned i;unsigned*p;unsigned*q;observe(sa,sb,999);for(i=0;i<64;i++){p=&sa[i];q=&sb[i];*p=i;*q=i+1;observe(p,q,i);}}
void local_body2(void){unsigned i;unsigned*p;unsigned*q;for(i=0;i<64;i++){p=&sa[i];q=&sb[i];*p=i;*q=i+1;sa[0]=i+7;observe(p,q,i);}}
void far_setup3(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&fa[i];q=&fb[i];r=&fc[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}}
void far_tail3(void){unsigned i;unsigned*p;unsigned*q;unsigned*r;for(i=0;i<64;i++){p=&fa[i];q=&fb[i];r=&fc[i];*p=i;*q=i+1;*r=i+2;observe3(p,q,r,i);}flush(fa,64);}
