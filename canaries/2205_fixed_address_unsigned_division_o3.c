// flags: -O3
typedef unsigned u32;
u32 result,zero;u32 a[80],b[80],c[80];
extern void consume(u32);extern void observe(u32*,u32*,u32*,u32);extern void flush(u32*,u32);
#define CLOCK (*(volatile u32*)0x800000f8)
u32 leaf3(void){return CLOCK/3;}
u32 leaf7(void){return CLOCK/7;}
u32 leaf10(void){return CLOCK/10;}
u32 leaf100(void){return CLOCK/100;}
u32 leaf400(void){return CLOCK/400;}
u32 leaf1000(void){return CLOCK/1000;}
void store400(void){result=CLOCK/400;}
void call400(void){consume(CLOCK/400);}
void stored_call(void){result=CLOCK/400;consume(result);}
void retained(u32 k){result=CLOCK/400;consume(k);}
u32 signed400(void){return (*(volatile int*)0x800000f8)/400;}
u32 pointer400(volatile u32*p){return *p/400;}
u32 shifted400(void){return (CLOCK>>2)/400;}
void cursor(void){u32 i;u32*p;u32*q;u32*r;result=CLOCK/400;zero=0;for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe(p,q,r,i);}flush(a,64);}
void cleared(void){u32 i;u32*p;u32*q;u32*r;result=CLOCK/400;zero=0;p=a;for(i=80;i!=0;i--){*p=0;p++;}p=b;for(i=80;i!=0;i--){*p=0;p++;}p=c;for(i=80;i!=0;i--){*p=0;p++;}for(i=0;i<64;i++){p=&a[i];q=&b[i];r=&c[i];*p=i;*q=i+1;*r=i+2;observe(p,q,r,i);}flush(a,64);}
u32 leaflow(void){return (*(volatile u32*)0x100)/400;}
u32 leafnegative(void){return (*(volatile u32*)0x80008004)/400;}
u32 leafother(void){return (*(volatile u32*)0x810000f8)/400;}
u32 bus_clock : 0x800000f8;
u32 declared400(void){return (u32)bus_clock/400;}
void declared_store(void){result=(u32)bus_clock/400;}
u32 cast400(void){return (u32)(int)CLOCK/400;}
u32 castsigned400(void){return (u32)(*(volatile int*)0x800000f8)/400;}
u32 narrow400(void){return (u32)(unsigned short)CLOCK/400;}
u32 narrowload400(void){return (u32)(*(volatile unsigned short*)0x800000f8)/400;}
u32 compound400(void){return (CLOCK+1)/400;}
void pair400(void){consume(CLOCK/400);consume((*(volatile u32*)0x810000f8)/400);}
