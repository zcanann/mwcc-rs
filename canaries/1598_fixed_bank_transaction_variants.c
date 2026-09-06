// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC00F000;
int transfer(void*, int, u32);
static int select_channel(u32 channel) {
    u32 value = registers[3];
    value &= 0x811;
    value |= 0x20 | (channel << 3);
    registers[3] = value;
    return 1;
}
static int deselect_channel(void) {
    registers[3] &= 0x811;
    return 1;
}
static int wait_ready(void) {
    while (registers[5] & 4) {}
    return 1;
}
int write_variant(u32 input) {
    int error = 0;
    u32 payload;
    if (!select_channel(4)) return 0;
    payload = (input & 0x0FFFFFFF) | 0x80000000;
    error |= !transfer(&payload, sizeof(payload), 1);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}

int read_variant(void* output) {
    int error = 0;
    u32 command;
    if (!select_channel(4)) return 0;
    command = 0x20000000;
    error |= !transfer(&command, 2, 1);
    error |= !wait_ready();
    error |= !transfer(output, 4, 0);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}
