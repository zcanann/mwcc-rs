// Materializing a 32-bit constant into the scratch register r0 can't fold the low
// half with `addi r0,r0,lo` — `addi rA=r0` means the literal 0, which would drop the
// `lis` high half. mwcc puts the `lis` in a separate register first: `lis t,hi;
// addi r0,t,lo`. This surfaces in `x != C` for a constant that doesn't fit 16 bits,
// whose branchless idiom stages C in r0.
int lc_ne_big(int x)   { return x != 1000000; }
int lc_ne_hex(int x)   { return x != 0x12345678; }
