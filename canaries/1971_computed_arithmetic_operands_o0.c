// flags: -Cpp_exceptions off -pragma "cats off"
unsigned byte_add(unsigned total, unsigned value) { return total + (unsigned char)((value >> 3) & 7); }
unsigned short_sub(unsigned total, unsigned value) { return total - (unsigned short)(value >> 3); }
unsigned signed_mul(unsigned total, unsigned value) { return total * (signed char)(value >> 5); }
unsigned pair_add(unsigned left, unsigned right) { return (unsigned short)(left >> 3) + (unsigned char)(right >> 5); }
unsigned pair_sub(unsigned left, unsigned right) { return (unsigned char)(left >> 3) - (unsigned short)(right >> 5); }
unsigned pair_mul(unsigned left, unsigned right) { return (signed char)(left >> 3) * (unsigned short)(right >> 5); }
unsigned table_add(unsigned total, const unsigned char* table, unsigned value) { return total + table[(unsigned char)((value >> 3) & 7)]; }
unsigned table_sub(unsigned total, const signed char* table, unsigned value) { return total - table[(unsigned char)((value >> 3) & 7)]; }
unsigned table_mul(unsigned total, const unsigned short* table, unsigned value) { return total * table[(unsigned char)((value >> 3) & 7)]; }
void stored(unsigned* out, unsigned value) { *out = *out + (unsigned char)(value >> 3); }
