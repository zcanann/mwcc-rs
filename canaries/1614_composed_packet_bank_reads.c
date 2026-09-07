// Melee-style nested packet reads: direct helper composition, retained query calls.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned long u32;
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
static int read_status(void* output) {
    int error = 0;
    u32 command;
    if (!select_channel(4)) return 0;
    command = 0x40000000;
    error |= !transfer(&command, 2, 1);
    error |= !wait_ready();
    error |= !transfer(output, 4, 0);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}

static int read_mailbox(void* output) {
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

typedef unsigned char u8;
extern int acquire(void);
extern void release(int);
static u32 mailbox;
static long remaining;
static u8 pending;
static void poll_mailbox(void) {
    u32 packet[2];
    read_status(packet);
    if (packet[0] & 1) {
        read_mailbox(packet);
        packet[0] &= ~0xe0000000;
        if ((packet[0] & 0x1f000000) == 0x1f000000) {
            mailbox = packet[0];
            remaining = packet[0] & 0x7fff;
            pending = 1;
        }
    }
}
int query(void) {
    int enabled;
    pending = 0;
    if (remaining == 0) {
        enabled = acquire();
        poll_mailbox();
    }
    release(enabled);
    return remaining;
}
