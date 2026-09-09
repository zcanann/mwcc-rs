typedef long long s64;
s64 signed_tests(s64 value, int a, int b) {
    if (a < b) value += 1;
    if (a <= b) value += 2;
    if (a > b) value += 4;
    if (a >= b) value += 8;
    if (a == b) value += 16;
    if (a != b) value += 32;
    return value;
}
s64 unsigned_tests(s64 value, unsigned a, unsigned b) {
    if (a < b) value += 1;
    if (a <= b) value += 2;
    if (a > b) value += 4;
    if (a >= b) value += 8;
    if (a == b) value += 16;
    if (a != b) value += 32;
    return value;
}
s64 thresholds(s64 value, int a, unsigned b) {
    if (-1 > a) value += 1;
    if (a >= 32768) value += 2;
    if (b < 0x80000000U) value += 4;
    if (65535U <= b) value += 8;
    return value;
}
s64 null_guard(s64 value, int *a, int *b) {
    if (!a || !b) return value;
    return value + *a - *b;
}
s64 logical_assign(s64 value, int x, int *y) {
    int z = 0;
    if (x && ((z = *y) > 2 || (z = 7) == 7)) return value + z;
    return value - z;
}
s64 stored_boolean(s64 value, int a, int b) {
    int test = a < b;
    if (test) value += test;
    return value + test;
}
static int probe(int *p) { int value = *p; *p = value + 1; return value; }
s64 call_guard(s64 value, int a, int *p) {
    if (a || probe(p) > 3) return value + *p;
    return value - *p;
}
