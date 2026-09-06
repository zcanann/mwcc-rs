// flags: -Cpp_exceptions off -pragma "cats off"
/* SDK BOOL is signed even when the register value and inserted field are unsigned. */
typedef int BOOL;
typedef unsigned int u32;
volatile u32 exi_registers[64] : 0xCC006800;

BOOL select_channel(u32 channel) {
    u32 state = exi_registers[10];
    state &= 0x405;
    state |= 0x80 | (channel << 4);
    exi_registers[10] = state;
    return 1;
}

BOOL select_high_mask(u32 channel) {
    u32 state = exi_registers[10];
    state &= 0x8001;
    state |= 0x80 | (channel << 4);
    exi_registers[10] = state;
    return 1;
}
