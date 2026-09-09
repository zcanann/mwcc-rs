typedef unsigned long long U64;
asm U64 stamped(unsigned char tag);
U64 asm_clock(U64 value, unsigned char tag) { return value + stamped(tag); }
U64 implicit_words(unsigned char first, U64 value, short second) {
    int token;
    token = legacy_words(first, second, value);
    return value + token;
}
typedef U64 (*NarrowCallback)(signed char, unsigned short);
U64 call_narrow(NarrowCallback callback, U64 value) {
    return callback((signed char)value, (unsigned short)(value >> 32)) + value;
}
typedef signed char (*ByteCallback)(unsigned short);
U64 call_byte(U64 value, ByteCallback callback) {
    return value + callback((unsigned short)value);
}
