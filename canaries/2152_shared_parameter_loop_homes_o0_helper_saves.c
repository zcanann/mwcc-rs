// flags: -O0 -use_lmw_stmw off
extern void observe(unsigned*,unsigned);
unsigned mark;
#define FILL(p) for(i=4;i!=0;i--){*p=0;p++;}
void one_use(unsigned*p,unsigned*q){unsigned i;*q=7;FILL(p);observe(p,4);}
void two_uses(unsigned*p,unsigned*q){unsigned i;*q=7;q[1]=9;FILL(p);observe(p,4);}
void three_parameters(unsigned*p,unsigned*q,unsigned yes){unsigned i;if(yes)*q=7;FILL(p);observe(p,4);}
void copied(unsigned*p,unsigned*q){unsigned i;unsigned*c=p;*q=7;FILL(c);observe(c,4);}
void loop_read(unsigned*p,unsigned*q){unsigned i;for(i=0;i<4;i++)p[i]=*q;observe(p,4);}
void unused(unsigned*p,unsigned*q,unsigned unused){unsigned i;*q=7;FILL(p);observe(p,4);}
void local_order(unsigned*p,unsigned*q){unsigned*c=p;unsigned i;*q=7;FILL(c);observe(c,4);}
void scalar(unsigned*p,unsigned x){unsigned i;mark=x;FILL(p);observe(p,4);}
void odd_frame(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i--){*p=0;p++;sum+=i;}observe(p,sum);}
void even_frame(unsigned*p,unsigned*q){unsigned sum=0;unsigned extra=1;unsigned i;*q=7;for(i=4;i!=0;i--){*p=0;p++;sum+=i;extra+=2;}observe(p,sum+extra);}
