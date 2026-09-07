// flags: -Cpp_exceptions off -pragma "cats off"
// Source int/long identities affect loop entry comparisons despite equal widths.
// The bank, register indices, trigger bit, and control-field shifts also vary.
typedef unsigned char u8;
typedef unsigned long u32;
typedef signed long s32;
typedef int BOOL;
#define TRUE 1
volatile u32 registers[32] : 0xCC00F000;
static BOOL wait_ready(void) {
    while (registers[10] & 4)
        ;
    return TRUE;
}

BOOL long_bound(void* buffer, s32 length, u32 mode) {
    u32 packed;
    u32 received;
    int iteration;

    if (mode) {
        packed = 0;
        for (iteration = 0; iteration < length; ++iteration) {
            u8* source_byte = (u8*)buffer + iteration;
            packed |= *source_byte << ((3 - iteration) << 3);
        }
        registers[11] = packed;
    }

    registers[10] = 4 | (mode << 3) | ((length - 1U) << 5);

    wait_ready();
    if (!mode) {
        u8* output_byte = (u8*)buffer;
        received = registers[11];
        for (iteration = 0; iteration < length; ++iteration) {
            *output_byte++ = received >> ((3 - iteration) << 3);
        }
    }

    return TRUE;
}

BOOL long_index(void* buffer, int length, u32 mode) {
    u32 packed;
    u32 received;
    long iteration;

    if (mode) {
        packed = 0;
        for (iteration = 0; iteration < length; ++iteration) {
            u8* source_byte = (u8*)buffer + iteration;
            packed |= *source_byte << ((3 - iteration) << 3);
        }
        registers[11] = packed;
    }

    registers[10] = 4 | (mode << 3) | ((length - 1U) << 5);

    wait_ready();
    if (!mode) {
        u8* output_byte = (u8*)buffer;
        received = registers[11];
        for (iteration = 0; iteration < length; ++iteration) {
            *output_byte++ = received >> ((3 - iteration) << 3);
        }
    }

    return TRUE;
}

BOOL long_both(void* buffer, s32 length, u32 mode) {
    u32 packed;
    u32 received;
    long iteration;

    if (mode) {
        packed = 0;
        for (iteration = 0; iteration < length; ++iteration) {
            u8* source_byte = (u8*)buffer + iteration;
            packed |= *source_byte << ((3 - iteration) << 3);
        }
        registers[11] = packed;
    }

    registers[10] = 4 | (mode << 3) | ((length - 1U) << 5);

    wait_ready();
    if (!mode) {
        u8* output_byte = (u8*)buffer;
        received = registers[11];
        for (iteration = 0; iteration < length; ++iteration) {
            *output_byte++ = received >> ((3 - iteration) << 3);
        }
    }

    return TRUE;
}
