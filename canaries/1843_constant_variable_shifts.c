// TEV-driven aggregate, select, and shift boundary coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
unsigned bit(unsigned n) { return 1u << n; }
unsigned computed_count(unsigned n) { return 3u << (n & 31); }
unsigned loaded_count(unsigned *p) { return 7u << *p; }
void enable(unsigned *p, unsigned n) { *p = *p | (1u << n); }
void disable(unsigned *p, unsigned n) { *p = *p & ~(1u << n); }
