// builds: GC/1.1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7
typedef volatile unsigned int vu32;
extern vu32 registers[16] : (0xCC006800);

void mask_interrupts(unsigned int mask);

void initialize_debugger(void) {
    mask_interrupts(0x18000);
    registers[10] = 0;
}
