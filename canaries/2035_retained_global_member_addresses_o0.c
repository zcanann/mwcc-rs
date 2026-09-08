// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Packet {
    unsigned short a_hi, a_lo;
    short a_delta;
    unsigned short b_hi, b_lo;
    short b_delta;
    unsigned short c_hi, c_lo;
    short c_delta;
};
struct Packet packet;
struct Large { short first; char padding[32766]; short second, third; };
struct Large large;
int input_a, input_b, input_c;
extern void sink(void *);
inline void write_value(int value, int *word, short *half) {
    if (value) {
        *word = value;
        *half = -value;
        return;
    }
    *word = 0;
    *half = 0;
}
void update(void) {
    write_value(input_a, (void *)&packet.a_hi, &packet.a_delta);
    write_value(input_b, (void *)&packet.b_hi, &packet.b_delta);
    write_value(input_c, (void *)&packet.c_hi, &packet.c_delta);
    sink(&packet);
}
void calls(void) {
    packet.a_delta = input_a;
    if (input_a) sink(&packet.a_hi);
    sink(&packet.b_hi);
    sink(&packet.c_hi);
}
void distant(void) {
    large.first = input_a;
    if (input_a) sink(&large.first);
    sink(&large.second);
    sink(&large.third);
}
