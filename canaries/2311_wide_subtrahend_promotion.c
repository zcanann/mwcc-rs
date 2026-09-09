typedef long long s64;
s64 sub_arg(s64 value, int n) { return value - n; }
s64 sub_load(s64 value, int *p) { return value - *p; }
s64 sub_cast(s64 value, int *p) { return value - (s64)*p; }
s64 sub_short(s64 value, short *p) { return value - *p; }
s64 sub_char(s64 value, signed char *p) { return value - *p; }
s64 sub_pair(s64 value, s64 n) { return value - n; }
s64 sub_unsigned(s64 value, unsigned n) { return value - n; }
s64 sub_constant(s64 value) { return value - (-1); }
s64 sub_compound(s64 value, int n) { value -= n; return value; }
s64 sub_sum(s64 value, int a, int b) { return value - (a + b); }
s64 sub_explicit_unsigned(s64 value, int n) { return value - (unsigned long long)n; }
s64 sub_shared(s64 value, int n, s64 *out) {
    s64 promoted = n;
    *out = promoted;
    return value - promoted;
}
s64 sub_repeated(s64 value, int n, s64 *out) {
    *out = (s64)n;
    return value - (s64)n;
}
s64 sub_local(s64 value, int n) { s64 promoted = n; return value - promoted; }
