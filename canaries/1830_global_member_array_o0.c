// Global member-array bases and local snapshots use shared value emission.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned lead; unsigned words[8]; float values[4]; signed short halves[4]; unsigned char bytes[8]; };
extern struct Context* p;
extern void* opaque;
unsigned read_word(unsigned i) { return p->words[i]; }
void write_word(unsigned i, unsigned v) { p->words[i]=v; }
void clear_word(unsigned i) { p->words[i]=0; }
void copy_float(unsigned i, const float* in) { p->values[i]=in[0]; }
unsigned through_cast(unsigned i) { return ((struct Context*)opaque)->words[i]; }
unsigned shadow(struct Context* p, unsigned i) { return p->words[i]; }
