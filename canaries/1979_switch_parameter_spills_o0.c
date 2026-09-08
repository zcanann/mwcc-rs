// flags: -Cpp_exceptions off -pragma "cats off"
extern void observe(unsigned);
extern void observe_signed(int);
void constant_choices(unsigned mode, unsigned value) {
    unsigned row = 9;
    switch (mode) {
    case 0: row = 13; break;
    case 1: row = 21; break;
    case 2: break;
    default: break;
    }
    observe(row ^ value);
}
void assigned_choices(unsigned mode, unsigned value) {
    unsigned row;
    switch (mode) {
    case 0: row = value + 11; break;
    case 1: row = value - 13; break;
    case 2: row = value ^ 63; break;
    default: row = value + 19; break;
    }
    observe(row);
}
void initialized_choices(unsigned mode, unsigned value) {
    unsigned row = value + 11;
    switch (mode) {
    case 0: row = 17; break;
    case 1: row = 23; break;
    default: break;
    }
    observe(row);
}
void signed_choices(int mode, int value) {
    int row;
    switch (mode) {
    case -2: row = value + 3; break;
    case -1: row = value - 5; break;
    default: row = value ^ 127; break;
    }
    observe_signed(row);
}
