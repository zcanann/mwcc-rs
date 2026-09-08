// flags: -O3,p
// Constant-trip pointer fills, reduced from BfBB __AXVPBInit.
extern void sink(void*);
static unsigned aligned[16] __attribute__((aligned(32)));
void fill_0(unsigned*p){unsigned i;for(i=0;i!=0;i--){*p=7;p++;}}
void fill_1(unsigned*p){unsigned i;for(i=1;i!=0;i--){*p=7;p++;}}
void fill_7(unsigned*p){unsigned i;for(i=7;i!=0;i--){*p=7;p++;}}
void fill_8(unsigned*p){unsigned i;for(i=8;i!=0;i--){*p=7;p++;}}
void fill_9(unsigned*p){unsigned i;for(i=9;i!=0;i--){*p=7;p++;}}
void fill_10(unsigned*p){unsigned i;for(i=10;i!=0;i--){*p=7;p++;}}
void fill_11(unsigned*p){unsigned i;for(i=11;i!=0;i--){*p=7;p++;}}
void fill_12(unsigned*p){unsigned i;for(i=12;i!=0;i--){*p=7;p++;}}
void fill_16(unsigned*p){unsigned i;for(i=16;i!=0;i--){*p=7;p++;}}
void fill_32(unsigned*p){unsigned i;for(i=32;i!=0;i--){*p=7;p++;}}
void fill_40(unsigned*p){unsigned i;for(i=40;i!=0;i--){*p=7;p++;}}
void fill_64(unsigned*p){unsigned i;for(i=64;i!=0;i--){*p=7;p++;}}
void fill_100(unsigned*p){unsigned i;for(i=100;i!=0;i--){*p=7;p++;}}
void fill_1024(unsigned*p){unsigned i;for(i=1024;i!=0;i--){*p=7;p++;}}
void bytes(unsigned char*p){unsigned i;for(i=32;i!=0;i--){*p=0xa5;p++;}}
void halves(unsigned short*p){unsigned i;for(i=40;i!=0;i--){*p=0x1234;p++;}}
unsigned count_return(unsigned*p){unsigned i;for(i=32;i!=0;i--){*p=11;p++;}return i;}
unsigned *pointer_return(unsigned*p){unsigned i;for(i=32;i!=0;i--){*p=11;p++;}return p;}
void conditional(unsigned*p,int yes){unsigned i;if(yes){for(i=32;i!=0;i--){*p=3;p++;}}sink(p);}
void callback(unsigned*p){unsigned i;for(i=64;i!=0;i--){*p=5;p++;}sink(p);}
void aligned_context(unsigned*p){unsigned i;for(i=64;i!=0;i--){*p=13;p++;}sink(aligned);}
void repeated(unsigned*p){unsigned i;for(i=32;i!=0;i--){*p=17;p++;}for(i=40;i!=0;i--){*p=19;p++;}sink(p);}
void nested(unsigned*p,int n){int j;unsigned i;for(j=0;j<n;j++){for(i=32;i!=0;i--){*p=23;p++;}}sink(p);}
void volatile_fill(volatile unsigned*p){unsigned i;for(i=32;i!=0;i--){*p=29;p++;}}
void fill_65(unsigned*p){unsigned i;for(i=65;i!=0;i--){*p=7;p++;}}
void fill_67(unsigned*p){unsigned i;for(i=67;i!=0;i--){*p=7;p++;}}
void fill_127(unsigned*p){unsigned i;for(i=127;i!=0;i--){*p=7;p++;}}
