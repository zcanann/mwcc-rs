// flags: -O1 -Cpp_exceptions off -pragma "cats off"
int input, output;
volatile int observed;
extern void mutate(void);
void guarded(void) {
    int quotient, value;
    quotient = input / 160;
    if (quotient) {
        value = input / 160;
        if (value > 20) value = 20;
        output = value;
    } else output = 0;
}
void replaced(void) {
    int quotient, value;
    quotient = input / 160;
    if (quotient) {
        input += 160;
        value = input / 160;
        output = value;
    } else output = 0;
}
void called(void) {
    int quotient, value;
    quotient = input / 160;
    if (quotient) {
        mutate();
        value = input / 160;
        output = value;
    } else output = 0;
}
void overwritten(void) {
    int quotient, value;
    quotient = input / 160;
    if (quotient) {
        quotient = 7;
        value = input / 160;
        output = quotient + value;
    } else output = 0;
}
void volatile_source(void) {
    int quotient, value;
    quotient = observed / 160;
    if (quotient) {
        value = observed / 160;
        output = value;
    } else output = 0;
}
void mixed_width(void) {
    short quotient;
    int value;
    quotient = input / 160;
    if (quotient) {
        value = input / 160;
        output = value;
    } else output = 0;
}
