// One narrow field update is followed by command/constant and command/state replay pairs on the
// same SDK fixed port. Build 163 retains the state base while materializing the high-word constant.
// builds: GC/1.2.5n
typedef struct ReplayState {
    unsigned padding[129];
    unsigned value;
} ReplayState;

typedef union ReplayPipe {
    unsigned char u8;
    unsigned u32;
    double f64;
} ReplayPipe;

extern ReplayState* const state;

void fixed_port_replay_update(unsigned char enabled) {
    ReplayState* data = state;
    data->value = (data->value & -524289) | ((int)enabled << 19);
    (*(volatile ReplayPipe*)0xCC008000).u8 = 0x61;
    (*(volatile ReplayPipe*)0xCC008000).u32 = 0xFE080000;
    (*(volatile ReplayPipe*)0xCC008000).u8 = 0x61;
    (*(volatile ReplayPipe*)0xCC008000).u32 = data->value;
}
