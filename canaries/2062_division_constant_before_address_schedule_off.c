// flags: -Cpp_exceptions off -pragma "cats off" -schedule off
struct Packet { int a, b, c, spare; };
struct Packet packet;
int input;
unsigned unsigned_input;
volatile int observed;
extern void sink(void *);
extern void sink_second(int, void *);
extern void mutate(void);
void signed_three(void) {
    int value, quotient;

    value = input;
    quotient = value / 3;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 3;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void signed_seven(void) {
    int value, quotient;

    value = input;
    quotient = value / 7;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 7;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void signed_large(void) {
    int value, quotient;

    value = input;
    quotient = value / 160;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 160;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void unsigned_seven(void) {
    unsigned value, quotient;

    value = unsigned_input;
    quotient = value / 7;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 7;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void volatile_source(void) {
    int value, quotient;

    value = observed;
    quotient = value / 160;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 160;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void preceding_store(void) {
    int value, quotient;
    packet.spare = 17;
    value = input;
    quotient = value / 160;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 160;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void preceding_call(void) {
    int value, quotient;
    mutate();
    value = input;
    quotient = value / 160;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 160;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink(&packet);
}
void second_argument(void) {
    int value, quotient;

    value = input;
    quotient = value / 160;
    if (quotient) {
        packet.a = value;
        packet.b = quotient;
        packet.c = value - quotient * 160;
    } else {
        packet.a = 0;
        packet.b = 0;
        packet.c = 0;
    }
    sink_second(input, &packet);
}

inline void fade_word(int *host, int *volume, short *delta) {
    int quotient = *host / 160;
    if (quotient) {
        if (quotient > 20) quotient = 20;
        if (quotient < -20) quotient = -20;
        *volume = *host;
        *host -= quotient * 160;
        *delta = quotient * -1;
        return;
    }
    *host = 0;
    *volume = 0;
    *delta = 0;
}
void inline_fade(void) {
    fade_word(&input, &packet.a, (short *)&packet.b);
    fade_word((int *)&unsigned_input, &packet.c, (short *)&packet.spare);
    sink(&packet);
}
