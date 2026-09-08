// flags: -O0
typedef struct { unsigned state,sync,count; unsigned *write; unsigned data[8]; } S;
typedef unsigned* WP;
S objects[64]; unsigned words[256]; S *cursor;
extern void observe(S*,unsigned);
#define RESET(p) (p)->state=0;(p)->sync=164;(p)->count=0;(p)->write=(unsigned*)(p)->data;(p)->data[0]=(p)->data[1]=(p)->data[2]=(p)->data[3]=(p)->data[4]=0
void reset(S*p){RESET(p);}
void initialize(void){unsigned i;S*p;for(i=0;i<64;i++){p=&objects[i];reset(p);observe(p,i);}}
void reset_return(S*p){RESET(p);}
S* returning(void){unsigned i;S*p;for(i=0;i<64;i++){p=&objects[i];reset_return(p);observe(p,i);}return p;}
void reset_volatile(S*p){RESET(p);}
void volatile_pointer(void){unsigned i;volatile WP p;for(i=0;i<64;i++){p=(unsigned*)&objects[i];reset_volatile((S*)p);observe((S*)p,i);}}
void reset_ordered(volatile S*p){RESET(p);}
void ordered(void){unsigned i;S*p;for(i=0;i<64;i++){p=&objects[i];reset_ordered(p);observe(p,i);}}
void reset_barrier(S*p){p->state=55;observe(p,77);p->count=66;}
void barrier(void){unsigned i;S*p;for(i=0;i<64;i++){p=&objects[i];reset_barrier(p);observe(p,i);}}
void reset_words(unsigned*p){p[0]=7;p[1]=9;p[2]=11;}
void scalar_pointer(void){unsigned i;unsigned*p;for(i=0;i<64;i++){p=&words[i*4];reset_words(p);observe((S*)p,i);}}
void reset_two(S*a,S*b){a->state=1;b->state=2;a->count=3;}
void dual_pointer(S*a,S*b){unsigned i;for(i=0;i<16;i++){reset_two(a,b);observe(a,i);a++;}}
