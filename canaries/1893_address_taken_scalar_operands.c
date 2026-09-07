// GXFrameBuf-driven discarded values, escaped scalar operands, and argument dependencies.
// flags: -Cpp_exceptions off -pragma "cats off"
extern void fill(unsigned int *, unsigned int *, unsigned int);
unsigned int product(unsigned int v) { unsigned int x, y; fill(&x, &y, v); return x*y; }
unsigned int sum(unsigned int v) { unsigned int x, y; fill(&x, &y, v); return x+y; }
unsigned int difference(unsigned int v) { unsigned int x, y; fill(&x, &y, v); return x-y; }
unsigned int masked(unsigned int v) { unsigned int x, y; fill(&x, &y, v); return x&y; }
unsigned int survivor(unsigned int v) { unsigned int x,y; fill(&x,&y,v); return x*v; }
unsigned int joined(unsigned int v) { unsigned int x,y; fill(&x,&y,v); return x|y; }
unsigned int toggled(unsigned int v) { unsigned int x,y; fill(&x,&y,v); return x^y; }
extern void fill_narrow(signed char *, short *, unsigned int);
int narrow_product(unsigned int v) { signed char x; short y; fill_narrow(&x,&y,v); return x*y; }
