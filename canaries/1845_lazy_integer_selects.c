// TEV-driven aggregate, select, and shift boundary coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
unsigned guarded(unsigned c, volatile unsigned *p) { return c ? 7 : *p + 3; }
unsigned selected(unsigned c, volatile unsigned *p, volatile unsigned *q) { return c ? *p + 5 : *q ^ 9; }
unsigned indexed(unsigned i, volatile unsigned *p) { return i >= 8 ? 7 : p[i]; }
unsigned narrow(unsigned c, unsigned a, unsigned b) { return c ? (unsigned char)(a + 1) : (unsigned short)(b + 2); }
unsigned leaf_selected(unsigned c, unsigned a, volatile unsigned *p) { return c ? a : *p + 7; }
