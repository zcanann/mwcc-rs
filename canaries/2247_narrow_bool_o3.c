// flags: -O3
typedef unsigned char u8;typedef signed char s8;typedef unsigned short u16;typedef signed short s16;typedef unsigned u32;
typedef struct S {u8 flags,other;u16 height,rows;s16 signed_half;u32 word;} S;
extern void sink2(u8,u8);extern u32 first(void);extern u32 second(void);
u8 mipmap(S*p){return (p->flags&1)==1;}
u8 bit_eq(S*p){return (p->flags&128)==128;}
s8 eq_bytes(S*p){return p->flags==p->other;}
u16 eq_scaled(S*p){return p->height==2*p->rows;}
s16 eq_signed(S*p){return p->signed_half==((int)p->other-128);}
u8 eq_wrap(u32 a,u32 b){return (a+1)==(b*2);}
u8 logical(S*p){return p->flags&&p->other;}
s8 not_flags(S*p){return !p->flags;}
void arg_scaled(S*p){sink2(p->flags,(p->height==2*p->rows)?(u8)1:0);}
void arg_pair(S*p){sink2(p->flags==p->other,p->height==2*p->rows);}
u8 eq_calls(void){return first()==second();}
s16 eq_parameter(S*p,int a){return p->signed_half==a;}
