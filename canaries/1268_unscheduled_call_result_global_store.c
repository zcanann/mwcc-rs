// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7
// flags: -sdata 0 -O0,p
short model;

short make_model(int data);
void use_model(short model);

void setup_model(void) {
    model = make_model(7);
    use_model(model);
}
