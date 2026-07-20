// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7
// flags: -sdata 0 -O0,p
short model;
int motions[2];

void create_model(int data, int *motions, int link);
void start_motion(short model, int motion, unsigned int attributes);
void set_layer(short model, int layer);

void create(void) {
    create_model(0x007b000f, motions, 0);
    start_motion(model, 1, 0x40000001);
    set_layer(model, 6);
}
