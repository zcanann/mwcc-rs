// Two inserts into one SDK state word are latency-scheduled with a fixed-port command/data pair.
// The second parameter's shifted value is prepared before the first member load.
// builds: GC/1.2.5n
typedef struct BitfieldState {
    unsigned short unused;
    unsigned short dirty;
    unsigned padding[30];
    unsigned value;
} BitfieldState;

typedef union BitfieldPipe {
    unsigned char u8;
    unsigned u32;
    double f64;
} BitfieldPipe;

extern BitfieldState* const state;

void fixed_port_bitfield_update(unsigned char width, int offsets) {
    BitfieldState* data = state;
    data->value = (data->value & -256) | ((int)width << 0);
    data->value = (data->value & -458753) | ((int)offsets << 16);
    (*(volatile BitfieldPipe*)0xCC008000).u8 = 0x61;
    (*(volatile BitfieldPipe*)0xCC008000).u32 = data->value;
    data->dirty = 0;
}

void fixed_port_shifted_bitfield_update(unsigned char size, int offsets) {
    BitfieldState* data = state;
    data->value = (data->value & -65281) | ((int)size << 8);
    data->value = (data->value & -3670017) | ((int)offsets << 19);
    (*(volatile BitfieldPipe*)0xCC008000).u8 = 0x61;
    (*(volatile BitfieldPipe*)0xCC008000).u32 = data->value;
    data->dirty = 0;
}
