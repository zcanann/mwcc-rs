// Global member-array bases and local snapshots use shared value emission.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned lead; unsigned words[8]; float values[4]; signed short halves[4]; unsigned char bytes[8]; };
extern struct Context* p;
void saved(unsigned i, unsigned v, unsigned* out) { unsigned old; (void)0; old=p->words[i]; p->words[i]=v; *out=old; }
void two_saved(unsigned i, unsigned v, unsigned* out) { unsigned a,b; a=p->words[i]; b=p->words[i+1]; p->words[i]=v; out[0]=a; out[1]=b; }
void narrow(unsigned i, unsigned v, int* out) { signed short old; old=p->halves[i]; p->halves[i]=v; *out=old; }
void floating(unsigned i, const float* in, float* out) { float old; old=p->values[i]; p->values[i]=in[0]; *out=old; }
void replace(unsigned i, struct Context* next, unsigned* out) { unsigned old; old=p->words[i]; p=next; *out=old; }
