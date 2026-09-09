// flags:
/* MD5-style rounds exercise more than 224 sequential virtual temporaries. */
typedef unsigned int u32;
#define ROUND(a, b, c, d, word, shift) \
    a += ((b & c) | (~b & d)) + word + 0x12345678u; \
    a = (a << shift) | (a >> (32 - shift)); \
    a += b
void unrolled_rounds(u32* state, const u32* input) {
    u32 a = state[0], b = state[1], c = state[2], d = state[3];
    ROUND(a, b, c, d, input[0], 7);
    ROUND(d, a, b, c, input[1], 12);
    ROUND(c, d, a, b, input[2], 17);
    ROUND(b, c, d, a, input[3], 22);
    ROUND(a, b, c, d, input[4], 7);
    ROUND(d, a, b, c, input[5], 12);
    ROUND(c, d, a, b, input[6], 17);
    ROUND(b, c, d, a, input[7], 22);
    ROUND(a, b, c, d, input[8], 7);
    ROUND(d, a, b, c, input[9], 12);
    ROUND(c, d, a, b, input[10], 17);
    ROUND(b, c, d, a, input[11], 22);
    ROUND(a, b, c, d, input[12], 7);
    ROUND(d, a, b, c, input[13], 12);
    ROUND(c, d, a, b, input[14], 17);
    ROUND(b, c, d, a, input[15], 22);
    ROUND(a, b, c, d, input[0], 7);
    ROUND(d, a, b, c, input[1], 12);
    ROUND(c, d, a, b, input[2], 17);
    ROUND(b, c, d, a, input[3], 22);
    ROUND(a, b, c, d, input[4], 7);
    ROUND(d, a, b, c, input[5], 12);
    ROUND(c, d, a, b, input[6], 17);
    ROUND(b, c, d, a, input[7], 22);
    ROUND(a, b, c, d, input[8], 7);
    ROUND(d, a, b, c, input[9], 12);
    ROUND(c, d, a, b, input[10], 17);
    ROUND(b, c, d, a, input[11], 22);
    ROUND(a, b, c, d, input[12], 7);
    ROUND(d, a, b, c, input[13], 12);
    ROUND(c, d, a, b, input[14], 17);
    ROUND(b, c, d, a, input[15], 22);
    ROUND(a, b, c, d, input[0], 7);
    ROUND(d, a, b, c, input[1], 12);
    ROUND(c, d, a, b, input[2], 17);
    ROUND(b, c, d, a, input[3], 22);
    ROUND(a, b, c, d, input[4], 7);
    ROUND(d, a, b, c, input[5], 12);
    ROUND(c, d, a, b, input[6], 17);
    ROUND(b, c, d, a, input[7], 22);
    ROUND(a, b, c, d, input[8], 7);
    ROUND(d, a, b, c, input[9], 12);
    ROUND(c, d, a, b, input[10], 17);
    ROUND(b, c, d, a, input[11], 22);
    ROUND(a, b, c, d, input[12], 7);
    ROUND(d, a, b, c, input[13], 12);
    ROUND(c, d, a, b, input[14], 17);
    ROUND(b, c, d, a, input[15], 22);
    ROUND(a, b, c, d, input[0], 7);
    ROUND(d, a, b, c, input[1], 12);
    ROUND(c, d, a, b, input[2], 17);
    ROUND(b, c, d, a, input[3], 22);
    ROUND(a, b, c, d, input[4], 7);
    ROUND(d, a, b, c, input[5], 12);
    ROUND(c, d, a, b, input[6], 17);
    ROUND(b, c, d, a, input[7], 22);
    ROUND(a, b, c, d, input[8], 7);
    ROUND(d, a, b, c, input[9], 12);
    ROUND(c, d, a, b, input[10], 17);
    ROUND(b, c, d, a, input[11], 22);
    ROUND(a, b, c, d, input[12], 7);
    ROUND(d, a, b, c, input[13], 12);
    ROUND(c, d, a, b, input[14], 17);
    ROUND(b, c, d, a, input[15], 22);
    state[0] = a; state[1] = b; state[2] = c; state[3] = d;
}
