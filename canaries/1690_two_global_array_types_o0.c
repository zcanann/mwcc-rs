// Candidate execution probe: independent indices, widths, and volatile reads.
// Fresh reference comparisons pending; captured word pairs share one index.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern unsigned char bytes[128];
extern unsigned short halves[128];
extern short signed_halves[128], signed_other[128];
extern volatile unsigned words[128], observed[128];
unsigned independent(unsigned i, unsigned j) { return words[i & 85] - observed[j & 3]; }
int signed_pair(unsigned i, unsigned j) { return signed_halves[i & 3] - signed_other[j & 1]; }
unsigned mixed(unsigned i, unsigned j) { return bytes[i & 3] ^ halves[j & 1]; }
