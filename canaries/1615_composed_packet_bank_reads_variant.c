// Variant changes bank, register slots, masks, command tags, and source names.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 bank[32] : 0xCC00F000;
int exchange(void*, int, u32);
static int select_bus(u32 channel) {
    u32 value = bank[3];
    value &= 0x811;
    value |= 0x20 | (channel << 3);
    bank[3] = value;
    return 1;
}
static int deselect_bus(void) {
    bank[3] &= 0x811;
    return 1;
}
static int wait_bus(void) {
    while (bank[5] & 4) {}
    return 1;
}
static int load_flags(void* output) {
    int error = 0;
    u32 command;
    if (!select_bus(4)) return 0;
    command = 0x20000000;
    error |= !exchange(&command, 2, 1);
    error |= !wait_bus();
    error |= !exchange(output, 4, 0);
    error |= !wait_bus();
    error |= !deselect_bus();
    return !error;
}

static int load_command(void* output) {
    int error = 0;
    u32 command;
    if (!select_bus(4)) return 0;
    command = 0x80000000;
    error |= !exchange(&command, 2, 1);
    error |= !wait_bus();
    error |= !exchange(output, 4, 0);
    error |= !wait_bus();
    error |= !deselect_bus();
    return !error;
}

typedef unsigned char u8;
extern int enter_guard(void);
extern void exit_guard(int);
static u32 captured;
static long length;
static u8 signaled;
static void check_input(void) {
    u32 packet[2];
    load_flags(packet);
    if (packet[0] & 4) {
        load_command(packet);
        packet[0] &= ~0x80000000;
        if ((packet[0] & 0x0f000000) == 0x0f000000) {
            captured = packet[0];
            length = packet[0] & 0x3fff;
            signaled = 7;
        }
    }
}
int available_input(void) {
    int enabled;
    signaled = 0;
    if (length == 0) {
        enabled = enter_guard();
        check_input();
    }
    exit_guard(enabled);
    return length;
}
