// Build 163 rounds a dynamic byte count to words, writes a fixed-port header, then uses an
// eight-store CTR loop plus a remainder CTR loop to flush zero words.
// builds: GC/1.2.5n
typedef struct FlushState {
    unsigned short unused;
    unsigned short flushed;
    unsigned short width;
    unsigned short height;
} FlushState;

typedef union FlushPipe {
    unsigned char u8;
    unsigned short u16;
    int s32;
    double f64;
} FlushPipe;

extern FlushState* const state;

void fixed_port_zero_fill(void) {
    unsigned i;
    unsigned short width = state->width;
    unsigned size = width * state->height;
    (*(volatile FlushPipe*)0xCC008000).u8 = 0x98;
    (*(volatile FlushPipe*)0xCC008000).u16 = width;
    for (i = 0; i < size; i += 4) {
        (*(volatile FlushPipe*)0xCC008000).s32 = 0;
    }
    state->flushed = 1;
}
