// Incoming word-argument execution probe; fresh reference objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern void pulse(void);
extern unsigned receive(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i, unsigned j);
unsigned survive(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i, unsigned j) { pulse(); return i + j; }
unsigned forward(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned i, unsigned j) { return receive(a,b,c,d,e,f,g,h,i,j); }
