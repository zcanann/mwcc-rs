// flags: -O0 -use_lmw_stmw off
extern void observe(unsigned*,unsigned);
void param_tie(unsigned*q,unsigned*p,unsigned*r){unsigned i;*r=7;for(i=4;i!=0;i--){*p=*q;q++;p++;}observe(p,*q);}
