// flags: -Cpp_exceptions off -pragma "cats off"
typedef int BOOL;
typedef unsigned int u32;
volatile u32 exi_registers[64] : 0xCC006800;

BOOL deselect_compound(void) {
    exi_registers[10] &= 0x405;
    return 1;
}

BOOL deselect_explicit(void) {
    exi_registers[10] = exi_registers[10] & 0x405;
    return 1;
}

BOOL deselect_high_mask(void) {
    exi_registers[10] &= 0x8001;
    return 1;
}
