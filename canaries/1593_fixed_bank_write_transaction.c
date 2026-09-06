// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
int transfer(void*, int, u32);
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
static int wait_ready(void) {
    while (registers[13] & 1) {}
    return 1;
}
int write_mailbox(u32 input) {
    int error = 0;
    u32 payload;
    if (!select_channel(4)) return 0;
    payload = (input & 0x1FFFFFFF) | 0xC0000000;
    error |= !transfer(&payload, sizeof(payload), 1);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}
