// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
void marker(u32);
static int clear_zero(void) {
    registers[10] &= 0x405;
    return 0;
}
static int wait_one(void) {
    while (registers[13] & 1) {}
    return 1;
}
int ordered_guards(int stop) {
    marker(1);
    if (stop) return 8;
    if (clear_zero()) return 9;
    return 3;
}
int guarded_poll(int stop) {
    if (stop) return 8;
    return wait_one();
}
int guard_value(int enabled) {
    if (enabled) return wait_one();
    return 9;
}
int stops_chain(void) {
    if (wait_one()) return 5;
    return clear_zero();
}
