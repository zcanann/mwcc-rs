// flags: -Cpp_exceptions off -pragma "cats off"
// Reduced transport caller: interleaved entry copies and four saved GPRs.
typedef unsigned int u32;
typedef unsigned char u8;
typedef int BOOL;
#define ALIGN_NEXT(x, n) (((x) + (n) - 1) & ~((n) - 1))
extern int OSDisableInterrupts(void);
extern void OSRestoreInterrupts(int);
extern int DBGReadStatus(void*);
extern int DBGWrite(u32, u32*, u32);
extern int DBGWriteMailbox(u32);
u8 SendCount = 0x80;
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
