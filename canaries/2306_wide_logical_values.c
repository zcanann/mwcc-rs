typedef long long S64;
typedef unsigned long long U64;
extern int witness(unsigned tag);
int wide_and(U64 value, unsigned tag) { return value && witness(tag); }
int wide_or(U64 value, unsigned tag) { return value || witness(tag); }
U64 conditional_assignment(U64 value, int flag) {
    int used;
    used = flag && (value = value + 7);
    return value + used;
}
S64 unary_values(S64 value, unsigned char byte) { return -value + ~byte + !value; }
U64 logical_loop(U64 value, int count) {
    int i;
    for (i = 0; i < count && value; i++) {
        if ((value & 3) || witness(i)) value -= 1;
        else break;
    }
    return value;
}
