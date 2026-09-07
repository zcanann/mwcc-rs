// flags: -Cpp_exceptions off -pragma "cats off"
extern void observe(unsigned);
void hinted(unsigned mode, unsigned value) {
    unsigned unused;
    unsigned row = 5;
    switch (mode) {
    case 0: row = 0; unused; break;
    case 1: row = 2; break;
    case 2: unused; break;
    default: unused; break;
    }
    observe(row + value);
}
void assigned(unsigned mode, unsigned value) {
    unsigned row;
    switch (mode) {
    case 0: row = value + 3; row; break;
    default: row = value + 7; row; break;
    }
    observe(row);
}
void initialized(unsigned mode, unsigned value) {
    unsigned row = value + 11;
    if (mode) row;
    observe(row);
}
