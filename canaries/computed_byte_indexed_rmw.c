void update_byte(unsigned char* bytes, int index, unsigned char value, int amount) {
    bytes[index / 2] |= value << amount;
}
