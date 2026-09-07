// flags: -Cpp_exceptions off -pragma "cats off"
// An address materialized only in a true arm cannot feed the false continuation.
#define PORT (*(volatile unsigned *)0xCC008000)
void conditional(unsigned flag, unsigned value) { unsigned reg = value; if (flag) { reg |= 3; PORT = reg; } PORT = value + 1; }
void nested(unsigned a, unsigned b, unsigned value) { if (a) { if (b) { PORT = value; } PORT = value + 1; } PORT = value + 2; }
void alternating(unsigned a, unsigned b, unsigned value) { if (a) { PORT = value; PORT = value + 1; } if (b) { PORT = value + 2; } PORT = value + 3; }
