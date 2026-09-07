// Pointer bases share mask-and-scale selection with global arrays.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Index { unsigned char padding[32]; unsigned int word; unsigned short half; unsigned char byte; };
unsigned int word(unsigned int n, unsigned int* p) { return p[n & 3]; }
unsigned int bits(unsigned int n, unsigned int* p) { return p[n & 85]; }
unsigned short half(struct Index* i, unsigned short* p) { return p[i->half & 15]; }
unsigned char byte(struct Index* i, unsigned char* p) { return p[i->byte & 3]; }
float real(struct Index* i, float* p) { return p[i->word & 85]; }
