// Typed word/pair operations: EABI padding, carry/borrow, conversions and stores.
typedef unsigned long long u64;
typedef long long s64;
extern void disturb(void);
extern void record(unsigned key, u64 value, unsigned tail);
u64 parameter_sum(unsigned key, u64 value, unsigned tail) {
    record(key, value, tail);
    return value + key + tail;
}
s64 add_signed(s64 value, int delta) {
    disturb();
    return value + delta;
}
u64 add_unsigned(u64 value, unsigned delta) {
    disturb();
    return value + delta;
}
u64 loaded_difference(const u64* p, const u64* q) {
    u64 a = *p;
    u64 b = *q;
    disturb();
    return a - b;
}
void pair_store(u64* p, u64 value, int delta) {
    u64 result = value + delta;
    disturb();
    *p = result;
}
unsigned narrowed_result(u64 value) {
    u64 result = value - 0x100000001ULL;
    disturb();
    return (unsigned)result;
}
u64 bitwise_mix(u64 a, u64 b) {
    u64 result = (a ^ b) | 0x0123456789abcdefULL;
    disturb();
    return result;
}
u64 literal_width(unsigned value) {
    u64 result = value + 1ULL;
    disturb();
    return result;
}
s64 signed_literal_width(int value) {
    s64 result = value + 1LL;
    disturb();
    return result;
}
u64 constant_carry(void) {
    return 0xffffffffULL + 1ULL;
}
