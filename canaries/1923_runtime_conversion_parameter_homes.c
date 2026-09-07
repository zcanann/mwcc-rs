// flags: -Cpp_exceptions off -pragma "cats off"
// Distinguish GC/1.1p1's overlapping O0 parameter homes from allocated homes.
unsigned direct(float x, unsigned n) { return (unsigned)x + n; }
unsigned direct_reverse(unsigned n, float x) { return (unsigned)x + n; }
unsigned assigned(float x, unsigned n) { unsigned r = x; return r + n; }
unsigned assigned_reverse(unsigned n, float x) { unsigned r = x; return r + n; }
unsigned double_direct(double x, unsigned n) { return (unsigned)x + n; }
unsigned double_assigned(double x, unsigned n) { unsigned r = x; return r + n; }
unsigned minus(float x, unsigned n) { unsigned r = x; return r - n; }
unsigned xor_value(float x, unsigned n) { unsigned r = x; return r ^ n; }
unsigned two_integers(float x, unsigned a, unsigned b) { unsigned r = x; return r + a + b; }
unsigned narrow(float x, unsigned short n) { unsigned r = x; return r + n; }
unsigned plain(float x) { unsigned r = x; return r; }
unsigned direct_plain(float x) { return (unsigned)x; }
extern unsigned consume(unsigned);
unsigned integer_control(unsigned x, unsigned n) { unsigned r = consume(x); return r + n; }
unsigned collision(float __mwcc_shared_spill_result_0, unsigned __mwcc_shared_spill_stack_0) { unsigned r = __mwcc_shared_spill_result_0; return r + __mwcc_shared_spill_stack_0; }
