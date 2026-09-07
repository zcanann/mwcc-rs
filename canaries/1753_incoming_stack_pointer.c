// Incoming word-argument execution probe; fresh reference objects pending.
// flags: -Cpp_exceptions off -pragma "cats off"
unsigned pointed(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned* p, unsigned index) { return p[index]; }
