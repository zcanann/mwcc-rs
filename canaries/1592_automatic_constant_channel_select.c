// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
void submit(u32);
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
void select_four(void) { select_channel(4); }
int select_four_result(void) { return select_channel(4); }
int select_four_and_clear(void) { select_channel(4); return deselect_channel(); }
void select_and_submit(u32 value) { select_channel(4); submit(value); }
