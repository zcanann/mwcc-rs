// Incoming word-argument execution probe; fresh reference objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
unsigned overwrite(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i) { i = 5; return i; }
unsigned guarded(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i) { if (a) i = 5; return i; }
unsigned increment(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i) { i += 7; return i; }
