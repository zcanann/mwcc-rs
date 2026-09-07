// flags: -Cpp_exceptions off -pragma "cats off"
// Reduced from Melee's DBGEXIImm: preserve the data pointer across packing
// loop backedges, control-register stores, polling, and the read-mode arm.
typedef unsigned char u8;
typedef unsigned int u32;
typedef int s32;
typedef int BOOL;
#define TRUE 1
volatile u32 registers[32] : 0xCC006800;
static BOOL wait_ready(void) {
    while (registers[13] & 1)
        ;
    return TRUE;
}

BOOL transfer(void* data, s32 byte_size, u32 write) {
    u32 writeVal;
    u32 readVal;
    int i;

    if (write) {
        writeVal = 0;
        for (i = 0; i < byte_size; ++i) {
            u8* nextWordPtr = (u8*)data + i;
            writeVal |= *nextWordPtr << ((3 - i) << 3);
        }
        registers[14] = writeVal;
    }

    registers[13] = 1 | (write << 2) | ((byte_size - 1U) << 4);

    wait_ready();
    if (!write) {
        u8* dataPtr = (u8*)data;
        readVal = registers[14];
        for (i = 0; i < byte_size; ++i) {
            *dataPtr++ = readVal >> ((3 - i) << 3);
        }
    }

    return TRUE;
}
