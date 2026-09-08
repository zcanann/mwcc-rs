// flags: -O0
typedef unsigned short u16;typedef unsigned long u32;typedef struct {u16 h0,l0,h1,l1,h2,l2;} Pair;
extern void observe(void*);
void split(Pair*a,Pair*b,void*p){a->h0=(u16)(((u32)p)>>16);a->l0=(u16)((u32)p);b->h0=(u16)(((u32)p)>>16);b->l0=(u16)((u32)p);}
void next(Pair*a,Pair*b,void*p){a->h0=(u16)(((u32)p+244)>>16);a->l0=(u16)((u32)p+244);b->h0=(u16)(((u32)p+244)>>16);b->l0=(u16)((u32)p+244);}
void previous(Pair*a,Pair*b,void*p){a->h0=(u16)(((u32)p-20)>>16);a->l0=(u16)((u32)p-20);b->h0=(u16)(((u32)p-20)>>16);b->l0=(u16)((u32)p-20);}
void three(Pair*a,Pair*b,void*p,void*q,void*r){a->h0=(u16)(((u32)p)>>16);a->l0=(u16)((u32)p);b->h0=(u16)(((u32)p)>>16);b->l0=(u16)((u32)p);a->h1=(u16)(((u32)q)>>16);a->l1=(u16)((u32)q);b->h1=(u16)(((u32)q)>>16);b->l1=(u16)((u32)q);a->h2=(u16)(((u32)r)>>16);a->l2=(u16)((u32)r);b->h2=(u16)(((u32)r)>>16);b->l2=(u16)((u32)r);}
void low_first(Pair*a,Pair*b,void*p){a->l0=(u16)(u32)p;a->h0=(u16)((u32)p>>16);b->l0=(u16)(u32)p;b->h0=(u16)((u32)p>>16);}
void volatile_split(volatile Pair*a,volatile Pair*b,void*p){a->h0=(u16)(((u32)p)>>16);a->l0=(u16)((u32)p);b->h0=(u16)(((u32)p)>>16);b->l0=(u16)((u32)p);}
void overwritten(Pair*a,Pair*b,void*p){a->h0=(u16)((u32)p>>16);a->h0=(u16)(u32)p;b->h0=(u16)((u32)p>>16);b->l0=(u16)(u32)p;}
void *returning(Pair*a,Pair*b,void*p){a->h0=(u16)(((u32)p)>>16);a->l0=(u16)((u32)p);b->h0=(u16)(((u32)p)>>16);b->l0=(u16)((u32)p);return p;}
void barrier(Pair*a,Pair*b,void*p){a->h0=(u16)((u32)p>>16);a->l0=(u16)(u32)p;observe(a);b->h0=(u16)((u32)p>>16);b->l0=(u16)(u32)p;}
void guarded(Pair*a,Pair*b,void*p,int yes){if(yes){a->h0=(u16)(((u32)p)>>16);a->l0=(u16)((u32)p);b->h0=(u16)(((u32)p)>>16);b->l0=(u16)((u32)p);}observe(a);}
void changed(Pair*a,Pair*b,void*p){a->h0=(u16)((u32)p>>16);a->l0=(u16)(u32)p;p=(char*)p+244;b->h0=(u16)((u32)p>>16);b->l0=(u16)(u32)p;}
