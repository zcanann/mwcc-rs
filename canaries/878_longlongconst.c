// Adding/subtracting a small CONSTANT to a single long-long parameter. mwcc materializes the
// 64-bit constant — LOW word in the next free GPR (r5), HIGH word in r0, or a single r0 when both
// words are equal — then `addc`/`adde`. `a - C` is lowered as `a + (-C)`, so `a - 1` becomes
// `li r0,-1; addc r4,r4,r0; adde r3,r3,r0` (both words -1 share r0). Restricted to one long-long
// parameter (a == result == r3:r4, r5 free) and li-sized words; a second parameter (whose dead
// registers mwcc would reuse), a wider constant, or a commuted `C + a` defers.
long long          addone(long long a)              { return a + 1; }
long long          addfive(long long a)             { return a + 5; }
long long          subone(long long a)              { return a - 1; }
long long          subhundred(long long a)          { return a - 100; }
unsigned long long uaddone(unsigned long long a)    { return a + 1; }
