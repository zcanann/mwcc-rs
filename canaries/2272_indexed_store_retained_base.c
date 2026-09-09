// flags:
/* Repeated indexed stores must retain their original pointer base. */
typedef unsigned int u32;
typedef unsigned char u8;
void encode_words(u8* output, const u32* input, u32 length) {
    u32 i, j;
    for (i = 0, j = 0; j < length; i++, j += 4) {
        output[j] = (u8)(input[i] & 0xff);
        output[j + 1] = (u8)((input[i] >> 8) & 0xff);
        output[j + 2] = (u8)((input[i] >> 16) & 0xff);
        output[j + 3] = (u8)((input[i] >> 24) & 0xff);
    }
}
u8* store_byte(u8* output, u32 index, u32 value) {
    output[index + 1] = value;
    return output;
}
u32* store_word(u32* output, u32 index, u32 value) {
    output[index + 1] = value;
    return output;
}
