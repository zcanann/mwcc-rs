// flags: -use_lmw_stmw off
extern void observe(unsigned*,unsigned);
unsigned a0[64];
unsigned a1[64];
unsigned a2[64];
unsigned a3[64];
unsigned a4[64];
unsigned a5[64];
unsigned a6[64];
unsigned a7[64];
unsigned a8[64];
void streams1(void){unsigned i;unsigned *p0;for(i=0;i<64;i++){p0=&a0[i];*p0=i+0;observe(p0,i);}}
void streams2(void){unsigned i;unsigned *p0;unsigned *p1;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];*p0=i+0;*p1=i+1;observe(p0,i);}}
void streams3(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];*p0=i+0;*p1=i+1;*p2=i+2;observe(p0,i);}}
void streams4(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];*p0=i+0;*p1=i+1;*p2=i+2;*p3=i+3;observe(p0,i);}}
void streams5(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;unsigned *p4;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];p4=&a4[i];*p0=i+0;*p1=i+1;*p2=i+2;*p3=i+3;*p4=i+4;observe(p0,i);}}
void streams6(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;unsigned *p4;unsigned *p5;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];p4=&a4[i];p5=&a5[i];*p0=i+0;*p1=i+1;*p2=i+2;*p3=i+3;*p4=i+4;*p5=i+5;observe(p0,i);}}
void streams7(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;unsigned *p4;unsigned *p5;unsigned *p6;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];p4=&a4[i];p5=&a5[i];p6=&a6[i];*p0=i+0;*p1=i+1;*p2=i+2;*p3=i+3;*p4=i+4;*p5=i+5;*p6=i+6;observe(p0,i);}}
void streams8(void){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;unsigned *p4;unsigned *p5;unsigned *p6;unsigned *p7;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];p4=&a4[i];p5=&a5[i];p6=&a6[i];p7=&a7[i];*p0=i+0;*p1=i+1;*p2=i+2;*p3=i+3;*p4=i+4;*p5=i+5;*p6=i+6;*p7=i+7;observe(p0,i);}}
volatile unsigned seed;
void parameter(unsigned bias){unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];*p0=i+bias+0;*p1=i+bias+1;*p2=i+bias+2;*p3=i+bias+3;observe(p0,i);}}
void eager(void){unsigned bias=seed;unsigned i;unsigned *p0;unsigned *p1;unsigned *p2;unsigned *p3;for(i=0;i<64;i++){p0=&a0[i];p1=&a1[i];p2=&a2[i];p3=&a3[i];*p0=i+bias+0;*p1=i+bias+1;*p2=i+bias+2;*p3=i+bias+3;observe(p0,i);}}
