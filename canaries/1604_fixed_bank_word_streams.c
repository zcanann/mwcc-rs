// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned long u32;
typedef signed long s32;
typedef unsigned char u8;
typedef int BOOL;
#define FALSE 0
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
// Reduced from the Melee transport: word-at-a-time calls, polling, and
// signed remaining-byte clamping in both transfer directions.
int read_words(u32 param1, u32* data, s32 byte_size) {
    BOOL error = FALSE;
    u32* dataPtr = (u32*)data;
    u32 writeValue;
    u32 readValue;

    if (!select_channel(4)) {
        return FALSE;
    }

    writeValue = (u32)(param1 << 8) & 0x1FFFC00 | 0x20000000;
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

int write_words(u32 param1, u32* data, s32 byte_size) {
    BOOL error = FALSE;
    u32* dataPtr = (u32*)data;
    u32 value;
    u32 nextWord;

    if (!select_channel(4)) {
        return FALSE;
    }

    value = (param1 & 0x1FFFC) << 8 | 0xA0000000;
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

