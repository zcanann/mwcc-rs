// Candidate execution probes for lookup sums; fresh compiler references pending.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned a[128], b[128], c[128], sink;
extern unsigned char bytes[128];
extern short halves[128];
extern volatile unsigned observed[128];
unsigned longer(unsigned i, unsigned cycles) { return a[i&3] + b[(i>>3)&3] + c[(i>>5)&3] + a[(i>>7)&3] + b[(i>>9)&3] + cycles + 0x87654321u; }
unsigned mixed(unsigned i) { return bytes[i&3] + halves[(i>>2)&3] + a[(i>>4)&3] + 5; }
unsigned volatile_sum(unsigned i) { return observed[i&3] + observed[(i>>2)&3] + observed[(i>>4)&3] + 7; }
unsigned repeated(unsigned i) { return a[i&3] + b[(i>>4)&3] + i + i + 0xfffffff9u; }
void store(unsigned i) { sink = a[i&3] + b[(i>>2)&3] + c[(i>>4)&3] + 11; }
