// flags: -Cpp_exceptions off -pragma "cats off"
struct Packet { int a, b, c, spare; };
struct Packet packet;
extern void sink(void *);
extern void mutate(void);
void branched(int value) {
    if (value) { packet.a = value; packet.b = value + 1; packet.c = value + 2; }
    else { packet.a = 3; packet.b = 4; packet.c = 5; }
    sink(&packet);
}
void linear(int value) {
    packet.a = value; packet.b = value + 1; packet.c = value + 2;
    sink(&packet);
}
void mixed(int value) {
    packet.a = value;
    if (value) { packet.b = value + 1; packet.c = value + 2; }
    else { packet.b = 4; packet.c = 5; }
    sink(&packet);
}
void looped(int count) {
    int i;
    for (i = 0; i < count; ++i) {
        packet.a += i; packet.b += i * 2; packet.c += i * 3;
    }
    sink(&packet);
}
void across_call(int value) {
    packet.a = value;
    mutate();
    packet.b = value + packet.a;
    packet.c = value + packet.b;
    sink(&packet);
}
inline void choose(int *out, int value) {
    if (value) { *out = value / 160; return; }
    *out = 0;
}
void inlined(int value) {
    choose(&packet.a, value);
    choose(&packet.b, value);
    choose(&packet.c, value);
    sink(&packet);
}
int input;
void divided(void) {
    int value = input;
    int quotient = value / 160;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 160;
    } else { packet.a = 0; packet.b = 0; packet.c = 0; }
    sink(&packet);
}
void nested(int value) {
    if (value > 0) {
        if (value > 10) { packet.a = 1; packet.b = 2; packet.c = 3; }
        else { packet.a = 4; packet.b = 5; packet.c = 6; }
    } else { packet.a = 7; packet.b = 8; packet.c = 9; }
    sink(&packet);
}
