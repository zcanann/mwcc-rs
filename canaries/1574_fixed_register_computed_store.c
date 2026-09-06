// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 exi_registers[64] : 0xCC006800;
u32 get_control(void);

void write_control(int byte_size, u32 write) {
    exi_registers[13] = 1 | (write << 2) | ((byte_size - 1U) << 4);
}

void write_sum(u32 left, u32 right) {
    exi_registers[13] = left + right;
}

void write_loaded(u32* data) {
    exi_registers[14] = *data;
}

void write_called(void) {
    exi_registers[13] = get_control();
}

void write_volatile(void) {
    exi_registers[13] = exi_registers[14] | 1;
}
