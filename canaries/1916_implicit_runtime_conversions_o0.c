// flags: -Cpp_exceptions off -pragma "cats off"
// Implicit scalar destinations expose runtime calls before frame planning.
unsigned initialized(float x, unsigned n) { unsigned y = x; return y + n; }
unsigned assigned(float x, unsigned n) { unsigned y; y = x; return y + n; }
unsigned loop_assigned(float x, unsigned n) { unsigned sum = 0; unsigned y; while (n) { y = x; sum += y; x += 0.5f; n--; } return sum; }
unsigned expression_assigned(float x, unsigned n) { unsigned y; unsigned z; z = (y = x); return y + z + n; }
unsigned returned(double x, unsigned n) { if (n) return x; return x + 0.5; }
unsigned switched(float x, unsigned n) { switch (n) { case 0: return x; case 1: return x + 0.5f; default: return x + 1.0f; } }
void pointer_store(unsigned *p, float x, unsigned n) { *p = x; p[1] = n; }
struct Sample { unsigned value; unsigned guard; };
void member_store(struct Sample *p, double x, unsigned n) { p->value = x; p->guard = n; }
