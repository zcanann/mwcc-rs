// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 exi_registers[64] : 0xCC006800;
void mask_interrupts(u32 mask);

void write_constant(void) {
    exi_registers[10] = 0;
}

void write_parameter(u32 value) {
    exi_registers[10] = value;
}

void initialize_after_call(void) {
    mask_interrupts(0x18000);
    exi_registers[10] = 0;
}
