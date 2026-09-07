// flags: -Cpp_exceptions off -pragma "cats off"
// Three-phase transport retries with composed status and mailbox transactions.
typedef unsigned int u32;
typedef unsigned char u8;
typedef int BOOL;
#define ALIGN_NEXT(x, n) (((x) + (n) - 1) & ~((n) - 1))
extern int enter_guard(void);
extern void exit_guard(int);
extern int write_stream(u32, u32*, u32);
static u8 sequence = 0x7F;
volatile u32 device[32] : 0xCC00F000;
int exchange(void*, int, u32);
static int select_channel(u32 channel) {
    u32 value = device[3];
    value &= 0x811;
    value |= 0x20 | (channel << 3);
    device[3] = value;
    return 1;
}
static int deselect_channel(void) {
    device[3] &= 0x811;
    return 1;
}
static int wait_ready(void) {
    while (device[5] & 4) {}
    return 1;
}
static int read_state(void* output) {
    int error = 0;
    u32 command;
    if (!select_channel(4)) return 0;
    command = 0x20000000;
    error |= !exchange(&command, 2, 1);
    error |= !wait_ready();
    error |= !exchange(output, 4, 0);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}

static int send_message(u32 input) {
    int error = 0;
    u32 payload;
    if (!select_channel(4)) return 0;
    payload = (input & 0x0FFFFFFF) | 0x80000000;
    error |= !exchange(&payload, sizeof(payload), 1);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}

int send_buffer(const void* data, u32 size) {
    u32 value;
    u32 state;
    BOOL saved_token;

    saved_token = enter_guard();
    do {
        read_state(&state);
    } while (state & 8);

    ++sequence;

    value = ((sequence & 4) ? 0x800 : 0);
    while (!write_stream(value | 0x34000, (u32*)data, ALIGN_NEXT(size, 8))) {}

    do {
        read_state(&state);
    } while (state & 8);

    value = (sequence << 8) | 0x07000000 | size;
    while (!send_message(value))
        ;

    do {
        while (!read_state(&state))
            ;
    } while (state & 8);

    exit_guard(saved_token);

    return -7;
}
