// flags: -O0 -use_lmw_stmw off
extern void observe(unsigned*,unsigned);
extern void observe3(unsigned*,unsigned,unsigned);
struct Pair {unsigned first, second;};
void swapped(unsigned*q,unsigned*p){observe(p,*q);}
void displaced(unsigned*q,unsigned*p){observe(p,q[3]);}
void member(struct Pair*q,unsigned*p){observe(p,q->second);}
void arithmetic(unsigned*q,unsigned*p){observe(p,*q+1);}
void both(unsigned*q,unsigned*p){observe(p,*q+*p);}
void third(unsigned*q,unsigned*p,unsigned*r){observe3(p,*q,*r);}
void crossed(unsigned*q,unsigned*p){observe3(p,*q,*p);}
void indexed(unsigned*q,unsigned*p,unsigned i){observe(p,q[i]);}
unsigned global_index;
void leaves(unsigned*q,unsigned*p){observe(p,(unsigned)q);}
void passthrough(unsigned*q,unsigned*p){observe(q,*q);}
void independent(unsigned*q,unsigned*p,unsigned*r){observe(p,*r);}
void literal(unsigned*q){observe((unsigned*)0,*q);}
void global_offset(unsigned*q,unsigned*p){observe(p,q[global_index]);}
void halfword(unsigned short*q,unsigned*p){observe(p,*q);}
void byte_value(signed char*q,unsigned*p){observe(p,*q);}
extern void volatile_observe(volatile unsigned*,unsigned,unsigned);
void volatile_crossed(volatile unsigned*q,volatile unsigned*p){volatile_observe(p,*q,*p);}
