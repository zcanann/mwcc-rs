// flags: -g
typedef struct { unsigned state,sync,count; unsigned *write; unsigned data[8]; unsigned tag; } S;
#define RESET(p,k) (p)->state=k;(p)->sync=164;(p)->count=k;(p)->write=(unsigned*)(p)->data;(p)->data[0]=(p)->data[1]=(p)->data[2]=(p)->data[3]=(p)->data[4]=k
#define TAIL(p,q,k) (q)->data[0]=(q)->data[1]=(p)->tag=k
void fallthrough(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}}
void diamond(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}else{p->tag=7;}}
void nonzero(S*p,S*q,unsigned x){RESET(p,7);if(x){TAIL(p,q,7);}}
void ordered(volatile S*p,volatile S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}}
void loaded(S*p,S*q,unsigned x){RESET(p,0);if(x){q->data[0]=0;p->tag=q->data[2];q->data[1]=0;}}
void distinct(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,7);}}
void single(S*p,S*q,unsigned x){p->tag=17;if(x)p->state=0;}
void nested(S*p,S*q,unsigned x){RESET(p,0);if(x){if(x==9){TAIL(p,q,0);}else{p->tag=7;}}}
unsigned returning(S*p,S*q,unsigned x){RESET(p,0);if(x){TAIL(p,q,0);}return p->tag+x;}
