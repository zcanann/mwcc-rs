// Incoming word-argument execution probe; fresh reference objects pending.
// flags: -Cpp_exceptions off -pragma "cats off"
extern void mutate(unsigned*);
unsigned address(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i) { mutate(&i); return i; }
