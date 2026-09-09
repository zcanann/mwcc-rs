// flags:
/* Four-byte packing used by the Dolphin MD5 decoder. */
typedef unsigned int u32;
typedef unsigned char u8;
u32 pack_le(const u8* input) {
    return ((u32)input[0]) | ((u32)input[1] << 8) |
        ((u32)input[2] << 16) | ((u32)input[3] << 24);
}
u32 pack_be(const u8* input) {
    return ((u32)input[0] << 24) | ((u32)input[1] << 16) |
        ((u32)input[2] << 8) | (u32)input[3];
}
void decode_words(u32* output, const u8* input, u32 length) {
    u32 i, j;
    for (i = 0, j = 0; j < length; i++, j += 4)
        output[i] = ((u32)input[j]) | ((u32)input[j + 1] << 8) |
            ((u32)input[j + 2] << 16) | ((u32)input[j + 3] << 24);
}
