/* Characterize allocator placement for one-use integer expression chains. */
int add_then_shift(int a) { return (a + 1) << 1; }
int add_then_multiply(int a) { return (a + 1) * 3; }
int add_then_or(int a) { return (a + 1) | 8; }
int add_then_xor(int a) { return (a + 1) ^ 8; }
int add_then_mask(int a) { return (a + 1) & 0xff; }
int add_then_negate(int a) { return -(a + 1); }
int shift_then_negate(int a) { return -(a << 2); }
int multiply_then_negate(int a) { return -(a * 3); }
int or_then_negate(int a) { return -(a | 8); }
int xor_then_negate(int a) { return -(a ^ 8); }
int mask_then_negate(int a) { return -(a & 0xff); }
int add_then_bitnot(int a) { return ~(a + 1); }
int multiply_negative_power(int a) { return a * -4; }

int local_add_then_shift(int a) {
    int value = a;
    value = value + 1;
    value = value << 1;
    return value;
}
int local_add_then_multiply(int a) {
    int value = a;
    value = value + 1;
    value = value * 3;
    return value;
}
int local_add_then_or(int a) {
    int value = a;
    value = value + 1;
    value = value | 8;
    return value;
}
int local_add_then_xor(int a) {
    int value = a;
    value = value + 1;
    value = value ^ 8;
    return value;
}
int local_add_then_mask(int a) {
    int value = a;
    value = value + 1;
    value = value & 0xff;
    return value;
}
int local_add_then_negate(int a) {
    int value = a;
    value = value + 1;
    value = -value;
    return value;
}
int second_parameter_local_chain(int pad, int a) {
    int value = a;
    value = value + 1;
    value = value << 1;
    return value;
}
int computed_initializer_local_chain(int a) {
    int value = a + 1;
    value = value << 1;
    return value;
}
int local_negative_power(int a) {
    int value = a;
    value = value * -4;
    return value;
}
