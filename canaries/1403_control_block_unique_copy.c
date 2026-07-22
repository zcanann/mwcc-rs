// A debug build keeps both arguments live through acquire/lock/copy/unlock/release.
// builds: GC/1.2.5n
// flags: -Cpp_exceptions off -O4,p -inline all -sdata 8 -sdata2 8
typedef signed long s32;
typedef unsigned char u8;

typedef struct Control {
    u8 bytes[128];
    void* work_area;
} Control;

typedef struct RecordTable {
    u8 records[2][12];
} RecordTable;

extern s32 acquire_control(s32 channel, Control** control);
extern RecordTable* lock_records(void);
extern void copy_bytes(void* destination, const void* source, unsigned long size);
extern void unlock_records(s32 commit);
extern s32 release_control(Control* control, s32 result);

s32 copy_unique_code(s32 channel, u8* output) {
    Control* control;
    s32 result;
    RecordTable* table;

    (void)0;
    if (0 > channel || channel >= 2) {
        return -128;
    }
    result = acquire_control(channel, &control);
    if (result < 0) {
        return result;
    }
    table = lock_records();
    copy_bytes(output, table->records[channel] + 4, 8);
    unlock_records(0);
    return release_control(control, 0);
}
