// flags: -Cpp_exceptions off -pragma "cats off"
struct Packet { int a, b, c, spare; };
struct Packet packet;
int input;
extern void sink_first(void *);
extern void sink_second(int, void *);
extern void sink_pair(void *, void *);
extern void sink_mixed(void *, int);
extern int source(void);
inline void write_value(int *target, int value) {
    if (value) {
        *target = value / 160;
        return;
    }
    *target = 0;
}
void first(void) {
    write_value(&packet.a, input);
    write_value(&packet.b, input);
    write_value(&packet.c, input);
    sink_first(&packet);
}
void parameter(int value) {
    write_value(&packet.a, value);
    write_value(&packet.b, value);
    write_value(&packet.c, value);
    sink_first(&packet);
}
void second(void) {
    write_value(&packet.a, input);
    write_value(&packet.b, input);
    write_value(&packet.c, input);
    sink_second(input, &packet);
}
void duplicate(void) {
    write_value(&packet.a, input);
    write_value(&packet.b, input);
    write_value(&packet.c, input);
    sink_pair(&packet, &packet);
}
void nested(void) {
    write_value(&packet.a, input);
    write_value(&packet.b, input);
    write_value(&packet.c, input);
    sink_mixed(&packet, source());
}
void thirds(int value) {
    if (value) {
        packet.a = value / 3;
        packet.b = value / 3;
        packet.c = value;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink_first(&packet);
}
void sevenths(int value) {
    if (value) {
        packet.a = value / 7;
        packet.b = value / 7;
        packet.c = value;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink_first(&packet);
}
