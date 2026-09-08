// flags:
typedef unsigned short u16;typedef unsigned long u32;typedef struct {u16 h0,l0,h1,l1,h2,l2;} Pair;
extern void observe(void*);
void split(u16*a,u16*b,void*p){a[0]=(u16)(((u32)p)>>16);a[1]=(u16)((u32)p);b[0]=(u16)(((u32)p)>>16);b[1]=(u16)((u32)p);}
void next(u16*a,u16*b,void*p){a[0]=(u16)(((u32)p+244)>>16);a[1]=(u16)((u32)p+244);b[0]=(u16)(((u32)p+244)>>16);b[1]=(u16)((u32)p+244);}
void previous(u16*a,u16*b,void*p){a[0]=(u16)(((u32)p-20)>>16);a[1]=(u16)((u32)p-20);b[0]=(u16)(((u32)p-20)>>16);b[1]=(u16)((u32)p-20);}
void three(u16*a,u16*b,void*p,void*q,void*r){a[0]=(u16)(((u32)p)>>16);a[1]=(u16)((u32)p);b[0]=(u16)(((u32)p)>>16);b[1]=(u16)((u32)p);a[2]=(u16)(((u32)q)>>16);a[3]=(u16)((u32)q);b[2]=(u16)(((u32)q)>>16);b[3]=(u16)((u32)q);a[4]=(u16)(((u32)r)>>16);a[5]=(u16)((u32)r);b[4]=(u16)(((u32)r)>>16);b[5]=(u16)((u32)r);}
void low_first(u16*a,u16*b,void*p){a[1]=(u16)(u32)p;a[0]=(u16)((u32)p>>16);b[1]=(u16)(u32)p;b[0]=(u16)((u32)p>>16);}
void volatile_split(volatile u16*a,volatile u16*b,void*p){a[0]=(u16)(((u32)p)>>16);a[1]=(u16)((u32)p);b[0]=(u16)(((u32)p)>>16);b[1]=(u16)((u32)p);}
void overwritten(u16*a,u16*b,void*p){a[0]=(u16)((u32)p>>16);a[0]=(u16)(u32)p;b[0]=(u16)((u32)p>>16);b[1]=(u16)(u32)p;}
void *returning(u16*a,u16*b,void*p){a[0]=(u16)(((u32)p)>>16);a[1]=(u16)((u32)p);b[0]=(u16)(((u32)p)>>16);b[1]=(u16)((u32)p);return p;}
void barrier(u16*a,u16*b,void*p){a[0]=(u16)((u32)p>>16);a[1]=(u16)(u32)p;observe(a);b[0]=(u16)((u32)p>>16);b[1]=(u16)(u32)p;}
void guarded(u16*a,u16*b,void*p,int yes){if(yes){a[0]=(u16)(((u32)p)>>16);a[1]=(u16)((u32)p);b[0]=(u16)(((u32)p)>>16);b[1]=(u16)((u32)p);}observe(a);}
void changed(u16*a,u16*b,void*p){a[0]=(u16)((u32)p>>16);a[1]=(u16)(u32)p;p=(char*)p+244;b[0]=(u16)((u32)p>>16);b[1]=(u16)(u32)p;}
