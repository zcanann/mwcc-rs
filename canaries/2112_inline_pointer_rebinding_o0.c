// flags: -O0
typedef struct { unsigned state,sync,count; unsigned *write; unsigned data[8]; } S;
typedef unsigned* WP;
S objects[64]; unsigned words[256]; S *cursor;
extern void observe(S*,unsigned);
void reset_global(S*p){p->state=11;cursor=&objects[1];p->count=22;}
void global_pointer(void){cursor=&objects[0];reset_global(cursor);observe(cursor,99);}
void reset_alias(S*p,S**slot){p->state=33;*slot=&objects[1];p->count=44;}
void alias_pointer(void){S*p;p=&objects[0];reset_alias(p,&p);observe(p,88);}
