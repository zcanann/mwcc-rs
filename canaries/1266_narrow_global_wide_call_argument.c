// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7
// flags: -sdata 0
short model;

void start_motion(short model, int motion, unsigned int attributes);
void set_layer(short model, int layer);

void begin_motion(void) {
    start_motion(model, 1, 0x40000001);
}

void select_layer(void) {
    set_layer(model, 6);
}
