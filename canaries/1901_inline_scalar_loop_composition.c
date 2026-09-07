// flags: -lang c
// Local loop state must be private to each retained inline invocation.
inline unsigned fold(unsigned input) {
    unsigned cursor = input;
    unsigned value = 0;
    while (cursor != 0) {
        value = value + (cursor & 15);
        cursor = cursor >> 4;
    }
    return value;
}
unsigned tail(unsigned x) { return fold(x); }
unsigned assigned(unsigned x) { unsigned y; y = fold(x); return y; }
unsigned guarded(unsigned x) { if (x == 0) return 77; return fold(x); }
unsigned repeated(unsigned x) { unsigned a; unsigned b; a = fold(x); b = fold(x >> 4); return a + b; }
unsigned changing(unsigned x) { unsigned cursor = x; unsigned value; value = fold(cursor); cursor = cursor >> 4; value = value + cursor; return value; }
unsigned collision(unsigned x) { unsigned __mwcc_inline_loop_result_0 = 91; unsigned value; value = fold(x); return value + __mwcc_inline_loop_result_0; }
unsigned outer(unsigned x) { unsigned i = 0; unsigned sum = 0; unsigned value; while (i < 3) { value = fold(x >> (i * 4)); sum = sum + value; i++; } return sum; }
inline unsigned fold_byte(unsigned char input) {
    unsigned cursor = input;
    unsigned value = 0;
    while (cursor != 0) { value = value + (cursor & 15); cursor = cursor >> 4; }
    return value;
}
unsigned narrowed(unsigned x) { return fold_byte(x); }
inline unsigned divide_after_loop(unsigned input) {
    unsigned cursor = input;
    unsigned quotient;
    while (cursor > 8) cursor = cursor >> 1;
    quotient = input / 2;
    return quotient + cursor;
}
unsigned signed_actual(int x) { return divide_after_loop(x); }
