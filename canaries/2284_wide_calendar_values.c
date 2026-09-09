/* Pair arithmetic and conditional definitions used by Dolphin time conversion. */
long long signed_quotient(long long value, long long divisor) { return value / divisor; }
unsigned long long unsigned_quotient(unsigned long long value, unsigned long long divisor) { return value / divisor; }
long long signed_remainder(long long value, long long divisor) { return value % divisor; }
unsigned long long unsigned_remainder(unsigned long long value, unsigned long long divisor) { return value % divisor; }
unsigned long long pair_product(unsigned long long a, unsigned long long b) { return a * b; }
long long normalized_remainder(long long value, unsigned period) {
    long long remainder = value % period;
    if (remainder < 0) remainder += period;
    return remainder;
}
long long selected_pair(long long a, long long b, int flag) {
    long long value;
    if (flag) value = a + b;
    else value = a - b;
    return value;
}
struct Parts { int before; int usec; int msec; int after; };
void subsecond_parts(long long ticks, unsigned clock, struct Parts *parts) {
    long long remainder = ticks % clock;
    if (remainder < 0) remainder += clock;
    parts->usec = (int)((remainder * 8) / (clock / 125000) % 1000);
    parts->msec = (int)(remainder / (clock / 1000) % 1000);
}
