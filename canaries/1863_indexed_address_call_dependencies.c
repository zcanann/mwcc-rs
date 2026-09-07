// Absolute member addresses and dependent pointer arguments.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned words[8]; };
extern struct Context* g;
extern void take(unsigned*, unsigned*);
void global_first(unsigned *p) { take(g->words, &p[1]); }
void local_first(unsigned *p) { take(&p[2], &p[1]); }
void variable_index(unsigned *p, unsigned i) { take(g->words, &p[i]); }
void global_address(unsigned *p) { take(&g->words[3], &p[2]); }
