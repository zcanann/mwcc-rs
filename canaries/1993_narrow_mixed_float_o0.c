// flags: -Cpp_exceptions off -pragma "cats off"
float get_bias(const unsigned* image) {
    short bits;
    bits = (int)((*image >> 9) & 255);
    return (signed char)bits / 32.0f;
}
float byte_scale(unsigned value) {
    short bits;
    bits = value;
    return (unsigned char)bits * 0.25f;
}
float half_scale(unsigned value) {
    int bits;
    bits = value;
    return (short)bits / 16.0f;
}
