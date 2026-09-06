// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
static int select_channel(u32 channel) {
    u32 value = registers[10];
    value &= 0x405;
    value |= 0x80 | (channel << 4);
    registers[10] = value;
    return 1;
}
static int deselect_channel(void) {
    registers[10] &= 0x405;
    return 1;
}
void select_only(u32 channel) { select_channel(channel); }
int deselect_result(void) { return deselect_channel(); }
int selected_transaction(u32 channel) {
    int error = 0;
    if (!select_channel(channel)) return 0;
    error |= !deselect_channel();
    return !error;
}
