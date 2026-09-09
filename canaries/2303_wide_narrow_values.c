typedef unsigned long long U64;
typedef long long S64;
struct Fields { signed char s8; unsigned char u8; short s16; unsigned short u16; };
extern U64 clock_word(void);
S64 narrow_fields(struct Fields *fields, S64 value) {
    value = value + fields->s8;
    value = value + fields->u8;
    value = value + fields->s16;
    return value + fields->u16;
}
U64 narrow_stores(struct Fields *fields, U64 value) {
    fields->s8 = (signed char)value;
    fields->u8 = (unsigned char)(value >> 8);
    fields->s16 = (short)(value >> 16);
    fields->u16 = (unsigned short)(value >> 32);
    return value + fields->u16;
}
S64 narrow_parameters(signed char first, S64 value, short second, unsigned char third, unsigned short fourth) {
    value = value + first;
    value = value + second;
    value = value + third;
    return value + fourth;
}
U64 narrow_after_call(unsigned char value, U64 addend) {
    U64 current;
    current = clock_word();
    value = (unsigned char)(current + value);
    return addend + value;
}
