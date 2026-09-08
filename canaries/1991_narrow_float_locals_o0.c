// flags: -Cpp_exceptions off -pragma "cats off"
unsigned packed_bias(float bias, unsigned word) {
    unsigned char byte;
    if (bias < -4.0f) bias = -4.0f;
    else if (bias >= 4.0f) bias = 3.99f;
    byte = 32.0f * bias;
    return (word & ~0x1fe00u) | ((unsigned)byte << 9);
}
int signed_half(float value, int addend) {
    short half;
    half = value;
    return half + addend;
}
unsigned unsigned_half(double value, unsigned addend) {
    unsigned short half;
    half = value;
    return half + addend;
}
int signed_byte(float value, int addend) {
    signed char byte;
    byte = value;
    return byte + addend;
}
