// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
typedef volatile unsigned int vu32;
extern vu32 registers[16] : (0xCC006800);

unsigned int select_debugger(unsigned int value) {
    unsigned int state = registers[10];
    state &= 0x405;
    state |= 0x80 | (value << 4);
    registers[10] = state;
    return 1;
}
