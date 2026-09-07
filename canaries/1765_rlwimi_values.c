// GX FIFO and rotate-insert lowering, with execution coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
extern unsigned first(void), second(void);
unsigned full(unsigned a, unsigned b) { return __rlwimi(a,b,0,0,31); }
unsigned wrap(unsigned a, unsigned b) { return __rlwimi(a,b,31,28,3); }
unsigned same(unsigned a) { return __rlwimi(a,a,7,5,23); }
unsigned computed(unsigned a, unsigned b) { return __rlwimi(a+17,b^0xABCDEF01,3,8,15); }
unsigned nested(unsigned a, unsigned b) { return __rlwimi(__rlwimi(a,b,5,0,7),a,11,24,31); }
