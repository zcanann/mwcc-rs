// Candidate execution probes: signed bias, independent inputs, widths and volatility.
// Fresh reference comparisons pending; the captured pair uses a +1 word index.
// flags: -Cpp_exceptions off -pragma "cats off"
unsigned independent(unsigned i, unsigned j, volatile unsigned* left, volatile unsigned* right) { return left[(i+7)&85] - right[j&3]; }
unsigned reverse(unsigned i, unsigned j, volatile unsigned* left, volatile unsigned* right) { return right[j&3] - left[(i+-3)&85]; }
int half(unsigned i, short* table) { return table[(i+1)&3] ^ table[i&3]; }
unsigned byte(unsigned i, unsigned char* table) { return table[i&3] + table[(1+i)&3]; }
unsigned standalone(unsigned i, volatile unsigned* table) { return table[(i+7)&85]; }
