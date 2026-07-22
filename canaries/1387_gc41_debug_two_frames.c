// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

extern int produce_value(void);

void gc41_debug_first_frame(int* destination) {
    *destination = produce_value();
}

void gc41_debug_second_frame(int* destination) {
    *destination = produce_value();
}
