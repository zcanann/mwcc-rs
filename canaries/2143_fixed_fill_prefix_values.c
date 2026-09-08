// flags:
extern void observe(unsigned*,unsigned);
unsigned mark;
unsigned buffer[128];
void global_zero(void){unsigned i;unsigned*p;mark=0;p=buffer;for(i=64;i!=0;i--){*p=0;p++;}observe(buffer,64);}
void global_seven(void){unsigned i;unsigned*p;mark=7;p=buffer;for(i=64;i!=0;i--){*p=7;p++;}observe(buffer,64);}
void pointer_zero(unsigned*p,unsigned*q){unsigned i;*q=0;for(i=64;i!=0;i--){*p=0;p++;}observe(p,64);}
void ordered(volatile unsigned*p,volatile unsigned*q){unsigned i;*q=7;for(i=64;i!=0;i--){*p=7;p++;}observe((unsigned*)p,64);}
void remainder(unsigned*p,unsigned*q){unsigned i;*q=7;for(i=100;i!=0;i--){*p=7;p++;}observe(p,100);}
void callback(unsigned*p,unsigned*q){unsigned i;*q=0;observe(q,99);for(i=64;i!=0;i--){*p=0;p++;}observe(p,64);}
void distinct(unsigned*p,unsigned*q){unsigned i;*q=3;for(i=64;i!=0;i--){*p=7;p++;}observe(p,64);}
void loaded(unsigned*p,unsigned*q){unsigned i;*q=0;mark=q[1];for(i=64;i!=0;i--){*p=0;p++;}observe(p,64);}
unsigned* result(unsigned*p,unsigned*q){unsigned i;*q=7;for(i=32;i!=0;i--){*p=7;p++;}return p;}
void conditional(unsigned*p,unsigned*q,unsigned yes){unsigned i;if(yes)*q=0;for(i=64;i!=0;i--){*p=0;p++;}observe(p,64);}
