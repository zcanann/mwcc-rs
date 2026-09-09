typedef long long S64;
typedef unsigned long long U64;
struct Coefficients { int tag; short rows[8][2]; };
S64 indexed_pair(const S64 *values, unsigned index, S64 bias) {
    return values[index] + bias;
}
S64 coefficient_sum(struct Coefficients *table, unsigned row, S64 bias) {
    return bias + table->rows[row][0] * table->rows[row][1];
}
U64 scatter(U64 *values, int index, U64 value) {
    values[index] = value;
    return values[index + 1] ^ value;
}
S64 stepped_shorts(short *out, const short *input, int count, int step, S64 sum) {
    int i;
    for (i = 0; i < count; i++) {
        sum += *input;
        *out = (short)sum;
        input = 1 + input;
        out += step;
    }
    return sum;
}
S64 member_address(struct Coefficients *table, unsigned row, S64 value) {
    short *p;
    p = &table->rows[row][1];
    *p = (short)value;
    return value + *p;
}
