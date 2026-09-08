// flags: -O2
typedef struct { unsigned state,sync,count; unsigned *write; unsigned data[8]; unsigned tag; } S;
extern void observe(S*,unsigned);
#define RESET(p,k) (p)->state=k;(p)->sync=164;(p)->count=k;(p)->write=(unsigned*)(p)->data;(p)->data[0]=(p)->data[1]=(p)->data[2]=(p)->data[3]=(p)->data[4]=k
#define TAIL(p,q,k) (q)->data[0]=(q)->data[1]=(p)->tag=k
void fallthrough(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}observe((S*)p,x);}
void diamond(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}else{p->tag=7;}observe((S*)p,x);}
void nonzero(S*p,S*q,unsigned x){RESET(p,7);if(x){TAIL(p,q,7);}observe((S*)p,x);}
void ordered(volatile S*p,volatile S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}observe((S*)p,x);}
void barrier(S*p,S*q,unsigned x){RESET(p,0);observe(p,x);if(x){TAIL(p,q,0);}observe((S*)p,x);}
void bypass(S*p,S*q,unsigned x){if(x==9)goto fill;RESET(p,0);fill:if(x){TAIL(p,q,0);}observe((S*)p,x);}
void loaded(S*p,S*q,unsigned x){RESET(p,0);if(x){q->data[0]=0;p->tag=q->data[2];q->data[1]=0;}observe((S*)p,x);}
void distinct(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,7);}observe((S*)p,x);}
