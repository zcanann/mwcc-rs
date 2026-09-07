// Computed integer equality, including GXFrameBuf masked-bit materialization.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct { unsigned char z; unsigned int value; } State;
void store_bit(State *p, unsigned int x) { p->z = (x & 16) == 16; }
void store_bits(State *p, unsigned int x) { p->z = (x & 20) == 20; }
void store_sum(State *p, unsigned int x, unsigned int y) { p->value = (x + y) == 17; }
void volatile_bit(volatile State *p) { p->z = (p->value & 16) == 16; }
void store_signed(State *p, int x) { p->value = (signed char)(x + 1) == -17; }
int narrow_mask(unsigned char x) { return (x & 16) == 16; }
int half_mask(unsigned short x) { return (x & 256) == 256; }
