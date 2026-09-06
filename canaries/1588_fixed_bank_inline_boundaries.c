// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
static int select_wide(u32 channel) {
    u32 value = registers[10];
    value &= 0xF405;
    value |= 0x280 | (channel << 7);
    registers[10] = value;
    return 1;
}
void discard_wide_status(u32 channel) { select_wide(channel); }
int wait_full_mask(int value) {
    while (registers[13] & 0xFFFFFFFF) {}
    return value;
}
void submit_pair(u32, u32);
void poll_then_pair(u32 first, u32 second) {
    while (registers[13] & 1) {}
    submit_pair(first, second);
}
