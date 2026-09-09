typedef unsigned long long U64;
extern U64 step_value(U64 value, unsigned count);
U64 fold_calls(U64 value, unsigned count) {
    while (count) {
        value = step_value(value, count);
        count = count - 1;
    }
    return value;
}
U64 wide_until(U64 value, unsigned count) {
    while (value) {
        if (count == 0) return value;
        value = value >> 1;
        count = count - 1;
    }
    return value;
}
U64 choose_return(U64 value, unsigned mode) {
    if (mode) return value + 7;
    else return value ^ 0x100000001ULL;
}
U64 rotate_pairs(U64 first, U64 second, unsigned count) {
    U64 temp;
    while (count) {
        temp = first;
        first = second;
        second = temp + count;
        count = count - 1;
    }
    return first ^ second;
}
