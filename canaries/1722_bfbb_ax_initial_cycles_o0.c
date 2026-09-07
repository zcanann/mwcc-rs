// Candidate execution probe; fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
// __AXSyncPBs's initial cycle expression, with the original constant arithmetic.
extern unsigned get_cycles(void);
unsigned initial_cycles(unsigned extra) { return (get_cycles() + 0x10000) - 0x55f0 + extra; }
unsigned reverse_cycles(unsigned extra) { return extra + ((get_cycles() + 0x10000) - 0x55f0); }
unsigned wide_xor(unsigned extra) { return (get_cycles() + 0x87654321u) ^ extra; }
