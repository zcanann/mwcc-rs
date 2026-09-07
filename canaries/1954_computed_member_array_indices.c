// flags: -Cpp_exceptions off -pragma "cats off"
struct Bank { unsigned prefix; unsigned words[8]; unsigned char bytes[16]; };
extern struct Bank *bank;
extern int __rlwimi(int, int, int, int, int);
void halved(unsigned i, unsigned value) {
    bank->words[i / 2] = __rlwimi(bank->words[i / 2], value, 24, 0, 7);
}
void loop_halved(void) {
    unsigned i;
    for (i = 0; i < 16; ++i)
        bank->words[i / 2] = __rlwimi(bank->words[i / 2], 0xf6 + i / 2, 24, 0, 7);
}
void wrapped(unsigned i, unsigned value) {
    bank->words[(i + 3) & 7] = bank->words[(i + 3) & 7] + value;
}
void narrow(unsigned i, unsigned value) {
    bank->bytes[i / 2] = value + i;
}
