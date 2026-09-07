// Masked global-array indices combine selection with the element-size shift.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Index { unsigned char padding[32]; unsigned int word; unsigned short half; unsigned char byte; };
extern unsigned int words[128];
extern unsigned short halves[128];
extern unsigned char bytes[128];
extern float floats[128];
unsigned int param_word(unsigned int n) { return words[n & 3]; }
unsigned int param_bits(unsigned int n) { return words[n & 85]; }
unsigned short param_half(unsigned int n) { return halves[n & 15]; }
unsigned char member_byte(struct Index* p) { return bytes[p->byte & 3]; }
unsigned short member_half(struct Index* p) { return halves[p->half & 15]; }
unsigned int member_word(struct Index* p) { return words[p->word & 85]; }
float member_float(struct Index* p) { return floats[p->byte & 3]; }
