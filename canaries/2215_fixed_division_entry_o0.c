// flags: -O0
typedef unsigned u32;
u32 result,zero;u32 a[80],b[80],c[80],d[80];
extern void consume(u32);extern void observe(u32*,u32*,u32*,u32);extern void observe4(u32*,u32*,u32*,u32*,u32);extern void flush(u32*,u32);
#define CLOCK (*(volatile u32*)0x800000f8)
u32 bus_clock : 0x800000f8;
void call3(void){consume(CLOCK/3);}
void call10(void){consume(CLOCK/10);}
void call400(void){consume(CLOCK/400);}
void call1000(void){consume(CLOCK/1000);}
void callnegative(void){consume((*(volatile u32*)0x80008004)/400);}
void calllow(void){consume((*(volatile u32*)0x100)/400);}
void declared_call(void){consume((u32)bus_clock/400);}
void retained(u32 k){result=CLOCK/400;consume(k);}
void preceding_call(void){consume(17);consume(CLOCK/400);}
void cursor(void){u32 i;u32*p;u32*q;u32*r;result=CLOCK/400;zero=0;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe(p,q,r,i);}flush(a,64);}
void cleared(void){u32 i;u32*p;u32*q;u32*r;result=CLOCK/400;zero=0;p=a;for(i=80;i!=0;i--){*p=0;p++;}p=b;for(i=80;i!=0;i--){*p=0;p++;}p=c;for(i=80;i!=0;i--){*p=0;p++;}for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe(p,q,r,i);}flush(a,64);}
void four_cleared(void){u32 i;u32*p;u32*q;u32*r;u32*s;result=CLOCK/400;zero=0;p=a;for(i=80;i!=0;i--){*p=0;p++;}p=b;for(i=80;i!=0;i--){*p=0;p++;}p=c;for(i=80;i!=0;i--){*p=0;p++;}p=d;for(i=80;i!=0;i--){*p=0;p++;}for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];s=&d[i];*p=i;*q=i+1;*r=i+2;*s=i+3;observe4(p,q,r,s,i);}flush(a,64);}
