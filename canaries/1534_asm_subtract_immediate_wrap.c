// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.5 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
/* The 2.4.x assembler wraps the negation; 2.3.x rejects these operands. */
asm void subtract_immediate_wrap(void) {
    nofralloc
    subi r3, r4, -32768
    subis r5, r6, -32768
    blr
}
