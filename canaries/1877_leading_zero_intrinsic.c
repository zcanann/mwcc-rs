// Integer leading-zero intrinsic, including zero and nested operands.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned int fetch(unsigned int);
unsigned int direct(unsigned int x) { return __cntlzw(x); }
unsigned int computed(unsigned int x, unsigned int y) { return __cntlzw(x ^ y); }
unsigned int loaded(volatile unsigned int *p) { return __cntlzw(*p); }
unsigned int doubled(unsigned int x) { return __cntlzw(x) + x; }
unsigned int nested(unsigned int x) { return __cntlzw(__cntlzw(x)); }
unsigned int zero(void) { return __cntlzw(0); }
unsigned int last_bit(unsigned int x) { return (31 - __cntlzw(x)) & 7; }
unsigned int called(unsigned int x) { return __cntlzw(fetch(x)); }
