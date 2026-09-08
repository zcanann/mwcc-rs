// flags: -use_lmw_stmw off
extern void observe(unsigned*,unsigned);
extern void flush(unsigned*,unsigned);
unsigned a0[8192], a1[8192], a2[64];
unsigned small[2];
unsigned initialized[64]={1};
void retained_three(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];*p0=i+0;*p1=i+1;*p2=i+2;observe(p0,i);}flush(a0,64);}
void retained_two(void){unsigned i;unsigned *p0;unsigned *p1;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];*p0=i+0;*p1=i+1;observe(p0,i);}flush(a1,64);}
void setup_only(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];*p0=i+0;*p1=i+1;*p2=i+2;observe(p0,i);}}
void call_before(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;observe(a0,999);for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];*p0=i+0;*p1=i+1;*p2=i+2;observe(p0,i);}flush(a2,64);}
void initialized_tail(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;for(i=0;i<64;i++){p0=&a0[i];p1=&initialized[i];p2=&a2[i];*p0=i+0;*p1=i+1;*p2=i+2;observe(p0,i);}flush(initialized,64);}
void small_data(void){unsigned i;unsigned *p0;unsigned *p1;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];*p0=i+0;*p1=i+1;small[i&1]=i+7;observe(p0,i);}flush(a0,64);}
