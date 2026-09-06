// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 exi_registers[64] : 0xCC006800;

void wait_without_status(void) {
    while (exi_registers[13] & 1) {
    }
}

int wait_until_clear(void) {
    while (exi_registers[13] & 1) {
    }
    return 1;
}

unsigned int wait_until_set(void) {
    while (!(exi_registers[13] & 0x80)) {
    }
    return 7;
}
