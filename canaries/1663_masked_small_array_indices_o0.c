// Small masked arrays use SDA21 and indexed loads on every compiler family.
// flags: -Cpp_exceptions off -pragma "cats off" -O0
struct Index { unsigned int value; };
extern unsigned char bytes[2];
extern unsigned short halves[2];
extern unsigned int words[2];
unsigned char byte(unsigned int i) { return bytes[i & 1]; }
unsigned short half(unsigned int i) { return halves[i & 1]; }
unsigned int word(unsigned int i) { return words[i & 1]; }
unsigned char member_byte(struct Index* p) { return bytes[p->value & 1]; }
unsigned short member_half(struct Index* p) { return halves[p->value & 1]; }
unsigned int member_word(struct Index* p) { return words[p->value & 1]; }
