// TEV-driven aggregate, select, and shift boundary coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
unsigned clamp(unsigned v) { return v >= 8 ? 0 : v; }
unsigned reassigned(unsigned v) { v = (v >= 8) ? 0 : v; return v; }
unsigned computed(unsigned c, unsigned a, unsigned b) { return c ? a + 7 : b ^ 0x1234; }
unsigned nested(unsigned c, unsigned a, unsigned b) { return c ? (a < 8 ? a : 0) : b + 3; }
void stored(unsigned *p, unsigned c, unsigned a) { *p = c ? 0 : a + 5; }
