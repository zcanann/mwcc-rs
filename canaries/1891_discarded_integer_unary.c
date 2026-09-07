// GXFrameBuf-driven discarded values, escaped scalar operands, and argument dependencies.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned int global;
extern volatile unsigned int observed;
extern unsigned int fetch(unsigned int);
void local_not(unsigned int x) { !x; }
void local_negative(int x) { -x; }
void local_complement(unsigned int x) { ~x; }
unsigned int reused(unsigned int x) { !x; return x+1; }
void global_not(void) { !global; }
void observed_not(void) { !observed; }
void loaded_not(volatile unsigned int *p) { !*p; }
void called_not(unsigned int x) { !fetch(x); }
