// flags: -Cpp_exceptions off -pragma "cats off"
volatile unsigned int word_registers[64] : 0xCC006800;
volatile unsigned short half_registers[32] : 0xCC005000;
volatile unsigned char byte_registers[32] : 0xCC004000;
unsigned int get_register_value(void);
unsigned int exchange_register_value(unsigned int value);

void write_word_result(void) {
    word_registers[13] = get_register_value();
}

void write_half_result(unsigned int value) {
    half_registers[3] = exchange_register_value(value);
}

void write_byte_result(unsigned int value) {
    byte_registers[7] = exchange_register_value(value);
}
