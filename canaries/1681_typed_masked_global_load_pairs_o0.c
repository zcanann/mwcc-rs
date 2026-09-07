// Mixed widths, member-derived indices, discontiguous masks, and volatile globals.
// flags: -Cpp_exceptions off -pragma "cats off" -O0
struct Index { unsigned padding[8], index; };
extern unsigned char bytes[128], byte_extra;
extern short halves[128], half_extra;
extern volatile unsigned words[128], word_extra;
unsigned read_bytes(unsigned index) { return bytes[index & 3] ^ byte_extra; }
int read_halves(struct Index* object) { return halves[object->index & 15] - half_extra; }
unsigned reverse_words(unsigned index) { return word_extra - words[index & 85]; }
unsigned member_words(struct Index* object) { return words[object->index & 3] + word_extra; }
