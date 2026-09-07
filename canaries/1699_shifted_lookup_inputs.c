// Candidate execution probes for fused right-shift/mask/element-scale selection.
// Fresh compiler-reference comparisons pending.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned left[128], right[128];
extern unsigned char bytes[128];
extern unsigned short halves[128];
unsigned signed_input(int value) { return left[(value >> 4) & 15] - right[value & 3]; }
unsigned high(unsigned value) { return left[(value >> 31) & 1] ^ right[(value >> 29) & 3]; }
unsigned narrow(unsigned char value) { return left[(value >> 4) & 15] + right[value & 3]; }
unsigned mixed(unsigned short value) { return bytes[(value >> 4) & 7] + halves[value & 3]; }
