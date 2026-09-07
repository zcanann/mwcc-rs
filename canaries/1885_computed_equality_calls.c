// Computed integer equality, including GXFrameBuf masked-bit materialization.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned int fetch(unsigned int);
extern unsigned char byte_value(unsigned int);
extern signed char signed_value(unsigned int);
int called(unsigned int x) { return fetch(x) == 17; }
int called_left(unsigned int x) { return 17 == fetch(x); }
int narrow(unsigned int x) { return byte_value(x) == 17; }
int signed_narrow(unsigned int x) { return signed_value(x) == -17; }
int called_computed(unsigned int x) { return (fetch(x) & 31) == 17; }
int conditional(int c, volatile unsigned int *p, unsigned int x) { return (c ? *p : x + 1) == 17; }
