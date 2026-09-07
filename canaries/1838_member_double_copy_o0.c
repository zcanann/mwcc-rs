// Indexed double copies retain both address and value across index evaluation.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned cursor, source; double values[8]; };
extern struct Context* p;
void copy(unsigned i, const double* in) { p->values[i]=in[0]; }
void at_cursor(const double* in) { p->values[p->cursor]=in[p->source]; }
void saved(unsigned i, const double* in, double* out) { double old; old=p->values[i]; p->values[i]=in[0]; *out=old; }
