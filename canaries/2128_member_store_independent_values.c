// flags:
typedef struct { unsigned state,sync,count; unsigned *write; unsigned data[8]; unsigned tag; } S;
extern void observe(S*,unsigned);
#define RESET(p) (p)->state=0;(p)->sync=164;(p)->count=0;(p)->write=(unsigned*)(p)->data;(p)->data[0]=(p)->data[1]=(p)->data[2]=(p)->data[3]=(p)->data[4]=0
void leading(S*p,S*q,unsigned x){p->write=p->data;p->tag=x;RESET(p);}
void ordered(volatile S*p,S*q,unsigned x){p->write=(unsigned*)p->data;p->tag=x;RESET(p);}
void alternating(S*p,S*q,unsigned x){p->write=p->data;p->tag=x;p->state=0;p->data[7]=x;p->sync=164;p->count=0;p->write=p->data;p->data[0]=p->data[1]=p->data[2]=p->data[3]=p->data[4]=0;}
void other_base(S*p,S*q,unsigned x){p->write=p->data;q->tag=x;RESET(p);}
void rebound(S*p,S*q,unsigned x){p->write=p->data;p->tag=x;p=q;RESET(p);}
void observed(S*p,S*q,unsigned x){p->write=p->data;p->tag=x;observe(p,x);RESET(p);}
void guarded(S*p,S*q,unsigned x){p->write=p->data;p->tag=x;if(x){RESET(p);}}
void base_value(S*p,S*q,unsigned x){p->write=p->data;p->tag=(unsigned)p;RESET(p);}
