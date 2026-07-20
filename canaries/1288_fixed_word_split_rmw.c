// A Dolphin fixed-address word register updated through separate local
// assignments. The compiler keeps the register page in r3 and the word in r0.
// builds: GC/1.2.5n
volatile unsigned fixed_words[0x100] : (0xCC006400);

static void fixed_word_split_rmw(void) {
    unsigned value;
    value = fixed_words[13];
    value |= 0x80000000;
    value &= ~1;
    fixed_words[13] = value;
}
