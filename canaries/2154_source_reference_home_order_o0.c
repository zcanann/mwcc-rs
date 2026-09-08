// flags: -O0
extern void observe(unsigned*,unsigned);
void hot_sum(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i--){sum+=i;sum+=i;sum+=i;*p++=sum;}observe(p,sum);}
void order_sum(unsigned*p,unsigned*q){unsigned i;unsigned sum=0;*q=7;for(i=4;i!=0;i--){*p=0;p++;sum+=i;}observe(p,sum);}
void hotter_sum(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i--){sum+=2;sum+=2;sum+=2;*p++=sum;}observe(p,sum);}
void sum_later(unsigned*p,unsigned*q){unsigned i;unsigned sum;*q=7;sum=0;for(i=4;i!=0;i--){sum+=2;sum+=2;sum+=2;*p++=sum;}observe(p,sum);}
void parameters(unsigned*p,unsigned*q,unsigned*r){unsigned i;*r=7;for(i=4;i!=0;i--){*p=*q;q++;q++;q++;p++;}observe(p,*q);}
void tied_locals(unsigned*p,unsigned*q){unsigned a=0;unsigned b=1;unsigned i;*q=7;for(i=4;i!=0;i--){*p++=a+b;a+=2;b+=3;}observe(p,a+b);}
void equal_locals(unsigned*p,unsigned*q){unsigned a=0;unsigned b=1;unsigned i;*q=7;for(i=4;i!=0;i--){*p++=a+b;}observe(p,a+b);}
void rank_two(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i--){sum+=2;*p++=sum;}observe(p,sum);}
void rank_none(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i--){*p++=sum;}observe(p,sum);}
void param_equal(unsigned*p,unsigned*q,unsigned*r){unsigned i;*r=7;for(i=4;i!=0;i--){*p=*q;q++;p++;}observe(p,*q);}
void none_later(unsigned*p,unsigned*q){unsigned sum;unsigned i;*q=7;sum=0;for(i=4;i!=0;i--){*p++=sum;}observe(p,sum);}
void step_assign(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i=i-1){*p++=sum;}observe(p,sum);}
void sum_assign(unsigned*p,unsigned*q){unsigned sum=0;unsigned i;*q=7;for(i=4;i!=0;i--){*p=0;p++;sum=sum+i;}observe(p,sum);}
void both_init(unsigned*p,unsigned*q){unsigned sum=0;unsigned i=4;*q=7;while(i!=0){*p++=sum;i--;}observe(p,sum);}
void post_value(unsigned*p,unsigned*q){unsigned i=0;*q=7;for(;i<4;){*p++=i++;}observe(p,i);}
