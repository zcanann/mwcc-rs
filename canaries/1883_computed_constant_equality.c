// Computed integer equality, including GXFrameBuf masked-bit materialization.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
int bit(u32 x) { return (x & 16) == 16; }
int bits(u32 x) { return (x & 20) == 20; }
int masked_constant(u32 x) { return (x & 20) == 16; }
int shifted(u32 x) { return (x >> 5) == 17; }
int added(u32 x,u32 y) { return (x + y) == 17; }
int negative(int x,int y) { return (x-y) == -17; }
int cast_byte(u32 x) { return (unsigned char)(x+1) == 17; }
int constant_left(u32 x) { return 16 == (x & 16); }
