// flags: -Cpp_exceptions off -pragma "cats off"
void masked_high(unsigned* out, unsigned* in) { *out = (*in & 0xfbffffff) + 0x80000000; }
void add_high(unsigned* out, unsigned* in) { *out = *in + 0x12340000; }
void subtract_high(unsigned* out, unsigned* in) { *out = *in - 0x43210000; }
