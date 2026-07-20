// MWCC accepts an explicit TBR operand on `mftb`, the form used by OSReset.
// builds: GC/1.2.5
asm void asm_explicit_timebase(void) {
    nofralloc
    mftb r3, 268
    blr
}
