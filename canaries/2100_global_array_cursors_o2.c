// flags: -O2
typedef struct { unsigned x; char pad[240]; } A;
typedef struct { unsigned x; char pad[60]; } B;
A aa[64]; B bb[64]; unsigned words[64];
extern void observe(A*,B*,unsigned);
void pair(void) {unsigned i; A *a; B*b; for(i=0;i<64;i++){a=&aa[i];b=&bb[i];a->x=i;b->x=i;observe(a,b,i);}}
void one(void) {unsigned i; A *a; for(i=0;i<64;i++){a=&aa[i];a->x=i;observe(a,0,i);}}
void chained(void) {unsigned i; A *a; B*b; for(i=0;i<64;i++){a=&aa[i];b=&bb[i];a->x=b->x=i;observe(a,b,i);}}
void conditional(void) {unsigned i; A *a; B*b; for(i=0;i<64;i++){a=&aa[i];b=&bb[i];if(i&1){a->x=i;b->x=i;}observe(a,b,i);}}
A *returned(void) {unsigned i; A *a; for(i=0;i<64;i++){a=&aa[i];a->x=i;observe(a,0,i);}return a;}
void changed(void) {unsigned i; A *a; for(i=0;i<32;i++){a=&aa[i];a++;a->x=i;observe(a,0,i);}}
void scalar(void) {unsigned i; unsigned *p; for(i=0;i<64;i++){p=&words[i];*p=i;observe(0,0,i);}}
void zero(void) {unsigned i; A *a; for(i=0;i<64;i++){a=&aa[i];a->x=0;observe(a,0,i);}}
void nonunit(void) {unsigned i; A *a; for(i=0;i<64;i+=2){a=&aa[i];a->x=i;observe(a,0,i);}}
void carried(void) {unsigned i; A *a; a=&aa[0];for(i=0;i<64;i++){a->x=i;observe(a,0,i);a++;}}
