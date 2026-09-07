// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
typedef int s32;
typedef unsigned char u8;
typedef int BOOL;
#define FALSE 0
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
// Vary the bank displacement, slots, masks, selection bits, command field
// geometry, source typedefs, and public names of the word-stream reduction.
int receive_packets(u32 param1, u32* data, s32 byte_size) {
    BOOL error = FALSE;
    u32* dataPtr = (u32*)data;
    u32 writeValue;
    u32 readValue;

    if (!select_channel(3)) {
        return FALSE;
    }

    writeValue = (u32)(param1 << 6) & 0x07FFF000 | 0x40000000;
    error |= !transfer((u8*)&writeValue, sizeof(writeValue), 1);
    error |= !wait_ready();

    while (byte_size != 0) {
        error |= !transfer((u8*)&readValue, sizeof(readValue), 0);
        error |= !wait_ready();

        *dataPtr++ = readValue;

        byte_size -= 4;
        if (byte_size < 0) {
            byte_size = 0;
        }
    }
    error |= !deselect_channel();

    return !error;
}

int send_packets(u32 param1, u32* data, s32 byte_size) {
    BOOL error = FALSE;
    u32* dataPtr = (u32*)data;
    u32 value;
    u32 nextWord;

    if (!select_channel(3)) {
        return FALSE;
    }

    value = (param1 & 0x1FFFC0) << 6 | 0x80000000;
    error = !transfer((u8*)&value, sizeof(value), 1);
    error |= !wait_ready();

    while (byte_size != 0) {
        nextWord = *dataPtr++;

        error |= !transfer((u8*)&nextWord, sizeof(nextWord), 1);
        error |= !wait_ready();

        byte_size -= 4;
        if (byte_size < 0)
            byte_size = 0;
    }

    error |= !deselect_channel();

    return !error;
}
