// Narrow stack arguments are right-aligned in their four-byte EABI slots.
// flags: -Cpp_exceptions off -pragma "cats off"
int byte(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned char value) { return value; }
int signed_byte(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, signed char value) { return value; }
int half(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned short value) { return value; }
int signed_half(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, short value) { return value; }
