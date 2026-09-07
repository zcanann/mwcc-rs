// Computed integer equality, including GXFrameBuf masked-bit materialization.
// flags: -Cpp_exceptions off -pragma "cats off"
int lower(unsigned int x, unsigned int y) { return (x - y) == -32768; }
int upper(unsigned int x, unsigned int y) { return (x + y) == 32767; }
int all_bits(unsigned int x, unsigned int y) { return (x ^ y) == -1; }
int narrow_cast(unsigned int x) { return (unsigned short)(x + 1) == 32767; }
int negative_cast(unsigned int x) { return (short)(x + 1) == -32768; }
int doubled(unsigned int x, unsigned int y) { return ((x + y) == 17) + x; }
int bit_reused(unsigned int x) { return ((x & 16) == 16) + x; }
