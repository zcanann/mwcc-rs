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
int read_mailbox(void* output) {
    int error = 0;
    u32 command;
    if (!select_channel(4)) return 0;
    command = 0x60000000;
    error |= !transfer(&command, 2, 1);
    error |= !wait_ready();
    error |= !transfer(output, 4, 0);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}
