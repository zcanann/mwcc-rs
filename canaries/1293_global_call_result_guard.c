// A call result stored into a global aggregate controls a trailing member
// update. The aggregate base and updated-field address survive the call in
// r31/r30; all three call arguments reuse one offset-field load.
// builds: GC/1.2.5n
typedef unsigned char u8;
typedef unsigned int u32;

typedef struct SramControl {
    u8 sram[64];
    u32 offset;
    u8 padding[8];
    int sync;
} SramControl;

extern int write_sram(void* buffer, u32 offset, u32 size);

static SramControl control;

void write_sram_callback(int channel, void* context) {
    control.sync = write_sram(
        control.sram + control.offset,
        control.offset,
        64 - control.offset
    );
    if (control.sync) {
        control.offset = 64;
    }
}
