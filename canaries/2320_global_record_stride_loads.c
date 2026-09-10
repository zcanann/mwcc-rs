struct Record { int first; int middle; int last; };
extern struct Record records[16];
struct Card { unsigned char prefix[184]; int xferred; unsigned char suffix[84]; };
extern struct Card cards[4];
int first(int i) { return records[i].first; }
int middle(int i) { return records[i].middle; }
int last(int i) { return records[i].last; }
int transferred(int i) { return cards[i].xferred; }
int retained(int i) { return records[i].middle+i; }
int next(int i) { return records[i+1].middle; }
int masked(unsigned i) { return records[i&7].last; }
int subtracted(int i, int x) { return x-records[i].last; }
int is_zero(int i) { return records[i].middle==0; }
void copy(int i, int *p) { *p=records[i].middle; }
extern void consume(int);
extern int query(void);
void argument(int i) { consume(records[i].last); }
int across_call(int i) { int x=records[i].middle; consume(i); return x; }
int after_call(int i) { consume(i); return records[i].last; }
int called_index(void) { return records[query()].first; }
struct Narrow { unsigned char a; signed char b; unsigned short c; short d; };
extern struct Narrow narrow[16];
int byte_field(int i) { return narrow[i].a; }
int signed_byte_field(int i) { return narrow[i].b; }
int half_field(int i) { return narrow[i].c; }
int signed_half_field(int i) { return narrow[i].d; }
struct FloatRecord { float a,b,c; };
extern struct FloatRecord floats[16];
float float_first(int i) { return floats[i].a; }
float float_last(int i) { return floats[i].c; }
void float_copy(int i,float *p) { *p=floats[i].b; }
struct DoubleRecord { double a,b,c; };
extern struct DoubleRecord doubles[16];
double double_first(int i) { return doubles[i].a; }
double double_last(int i) { return doubles[i].c; }

struct Bytes { unsigned char a,b,c; };
extern struct Bytes tiny[2];
extern struct Bytes triples[16];
int tiny_first(int i) { return tiny[i].a; }
int tiny_last(int i) { return tiny[i].c; }
int triple_last(int i) { return triples[i].c; }
int two_fields(int i) { return records[i].first+records[i].last; }
struct Large { unsigned char first; unsigned char padding[32767]; int value; };
extern struct Large large[2];
int large_first(int i) { return large[i].first; }
int large_last(int i) { return large[i].value; }
