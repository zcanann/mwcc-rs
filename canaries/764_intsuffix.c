// Integer literals may carry type-suffix letters (u/U/l/L and combinations: UL, LL,
// ULL) — pervasive in the game's headers (0x1234U, [16U], masks). On this 32-bit
// target they're type hints only and don't change the value, so the lexer consumes
// and drops them. Previously a suffix lexed as a stray identifier and derailed the
// parse (e.g. "expected ParenClose, found Identifier(\"U\")").
int suf_hex(void)        { return 0x10U; }
int suf_dec_mixed(void)  { return 5U + 3L; }
int suf_mask(int x)      { return x & 0xFFFFU; }
int suf_local(void)      { int x = 7UL; return x; }
int suf_combo(void)      { return 0x1000UL + 2U; }
int suf_shift(int x)     { return x << 4U; }
