// Two narrow parameters update adjacent bits of one indexed SDK state word. Build 163 retains two
// copies of the indexed address while scheduling the updated word into a fixed-port write.
// builds: GC/1.2.5n
typedef struct IndexedState {
    unsigned short unused;
    unsigned short dirty;
    unsigned padding[45];
    unsigned words[8];
} IndexedState;

typedef union IndexedPipe {
    unsigned char u8;
    unsigned u32;
    double f64;
} IndexedPipe;

extern IndexedState* const state;

void fixed_port_indexed_bitfield(int index, unsigned char first, unsigned char second) {
    IndexedState* data = state;
    data->words[index] = (data->words[index] & -262145) | ((int)first << 18);
    data->words[index] = (data->words[index] & -524289) | ((int)second << 19);
    (*(volatile IndexedPipe*)0xCC008000).u8 = 0x61;
    (*(volatile IndexedPipe*)0xCC008000).u32 = data->words[index];
    data->dirty = 0;
}
