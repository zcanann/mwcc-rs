// Independent virtual homes for narrowed bitwise subtrees.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned* output;
unsigned merge(unsigned old, unsigned x, unsigned y) { return (old & 0xffff00ffu) | ((unsigned)(unsigned char)(x+y) << 8); }
unsigned and_tree(unsigned old, unsigned x, unsigned y) { return (old+7) & ((unsigned)(unsigned short)(x^y) << 3); }
unsigned xor_tree(unsigned old, unsigned x, unsigned y) { return (old*5) ^ ((unsigned)(unsigned char)(x+y) << 9); }
void stored(unsigned old, unsigned x, unsigned y) { *output=(old & 0xffff00ffu) | ((unsigned)(unsigned char)(x+y) << 8); }
unsigned aliases(unsigned value) { return (value & 0xffff00ffu) | ((unsigned)(unsigned char)(value+5) << 8); }
unsigned dynamic_shift(unsigned value, unsigned amount) { return (unsigned)(unsigned char)(value+3) << (amount&31); }
unsigned signed_lane(unsigned old, unsigned x) { return (old & 0xffff0000u) | (unsigned)(signed short)x; }
