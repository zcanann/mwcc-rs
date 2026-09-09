// Scoped integer DAG reuse, including the inlined Dolphin leap-day arithmetic.
int doubled_increment(int value) { return (value + 1) + (value + 1); }
unsigned nested_repeat(unsigned value) {
    return ((value + 1) * (value + 1)) + ((value + 1) * (value + 1));
}
unsigned shared_sign(unsigned value) { return (value + (value >> 31)) ^ (value >> 31); }
int leap_days(int year) {
    if (year < 1) return 0;
    return (year + 3) / 4 - (year - 1) / 100 + (year - 1) / 400;
}
unsigned separate_domains(unsigned a, unsigned b) {
    return ((a + 7) ^ (b - 3)) + ((a + 7) & (b - 3));
}
int shared_compare(int value, int limit) {
    return ((value - 1) / 100 + (value - 1) / 400) < limit;
}

int shared_remainder(int value) {
    return value + ((value % 4) ^ (value % 4 + 1));
}
int reused_narrow(int value) {
    return (short)(value + 1) * (short)(value + 1);
}
