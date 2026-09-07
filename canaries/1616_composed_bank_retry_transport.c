// flags: -Cpp_exceptions off -pragma "cats off"
// Three-phase transport retries with composed status and mailbox transactions.
typedef unsigned long u32;
typedef unsigned char u8;
typedef int BOOL;
#define ALIGN_NEXT(x, n) (((x) + (n) - 1) & ~((n) - 1))
extern int OSDisableInterrupts(void);
extern void OSRestoreInterrupts(int);
extern int DBGWrite(u32, u32*, u32);
static u8 SendCount = 0x80;
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
static int DBGReadStatus(void* output) {
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

static int DBGWriteMailbox(u32 input) {
    int error = 0;
    u32 payload;
    if (!select_channel(4)) return 0;
    payload = (input & 0x1FFFFFFF) | 0xC0000000;
    error |= !transfer(&payload, sizeof(payload), 1);
    error |= !wait_ready();
    error |= !deselect_channel();
    return !error;
}

int DBWrite(const void* data, u32 size) {
    u32 value;
    u32 busyFlag;
    BOOL enabled;

    enabled = OSDisableInterrupts();
    do {
        DBGReadStatus(&busyFlag);
    } while (busyFlag & 2);

    ++SendCount;

    value = ((SendCount & 1) ? 0x1000 : 0);
    while (!DBGWrite(value | 0x1C000, (u32*)data, ALIGN_NEXT(size, 4))) {}

    do {
        DBGReadStatus(&busyFlag);
    } while (busyFlag & 2);

    value = (SendCount << 0x10) | 0x1F000000 | size;
    while (!DBGWriteMailbox(value))
        ;

    do {
        while (!DBGReadStatus(&busyFlag))
            ;
    } while (busyFlag & 2);

    OSRestoreInterrupts(enabled);

    return 0;
}
