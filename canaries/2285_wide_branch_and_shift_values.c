struct Comparisons { int lt, le, gt, ge, eq, ne; };
void compare_pairs(long long a, long long b, struct Comparisons *out) {
    out->lt = a < b; out->le = a <= b; out->gt = a > b;
    out->ge = a >= b; out->eq = a == b; out->ne = a != b;
}
void compare_unsigned(unsigned long long a, unsigned long long b, struct Comparisons *out) {
    out->lt = a < b; out->le = a <= b; out->gt = a > b;
    out->ge = a >= b; out->eq = a == b; out->ne = a != b;
}
unsigned long long shift_cross(unsigned long long value) {
    return ((value << 7) ^ (value >> 19)) + ((value << 32) ^ (value >> 32))
        + ((value << 63) ^ (value >> 63));
}
long long signed_shift_mix(long long value) {
    return (value >> 1) + (value >> 32) + (value >> 63);
}
extern long long change_value(long long);
long long nested_merge(long long a, long long b, int flag) {
    long long result = a;
    if (flag) {
        if (a < b) result = change_value(a + b);
        else result = a - b;
    } else result = change_value(b);
    return result + a;
}
