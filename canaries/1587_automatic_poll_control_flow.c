// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
static int ready(void) {
    while (registers[13] & 1) {}
    return 1;
}
int guarded_wait(int enabled) {
    if (!enabled) return 0;
    return ready();
}
int short_circuit_wait(int enabled) { return enabled && ready(); }
int shadowed_bank(volatile u32 *registers) {
    ready();
    return registers[0];
}
void repeated_wait(void) { ready(); ready(); }
