// Global member-array bases and local snapshots use shared value emission.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned lead; unsigned words[8]; float values[4]; signed short halves[4]; unsigned char bytes[8]; };
typedef struct Context* ContextPointer;
extern volatile ContextPointer p;
void saved(unsigned i, unsigned v, unsigned* out) { unsigned old; old=p->words[i]; p->words[i]=v; *out=old; }
void twice(unsigned i, unsigned* out) { unsigned a,b; a=p->words[i]; b=p->words[i]; out[0]=a; out[1]=b; }
void clear_word(unsigned i) { p->words[i]=0; }
void copy_float(unsigned i, const float* in) { p->values[i]=in[0]; }
