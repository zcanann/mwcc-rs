// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Packet {
    int word0, word1, word2;
    short half0, half1, half2;
    unsigned char byte0, byte1, byte2;
};
struct Packet packet;
int selector, result;
extern void sink(void *);
inline void sample(int flag, int *word, short *half, unsigned char *byte) {
    if (flag) {
        *word += 1;
        *half += 1;
        *byte += 1;
        result = word[1] + half[1] + byte[1];
        return;
    }
    result = word[-1] + half[-1] + byte[-1];
}
void indexed(void) {
    sample(selector, &packet.word1, &packet.half1, &packet.byte1);
    sink(&packet);
}
