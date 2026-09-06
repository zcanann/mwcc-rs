/* Matrix array loops subtract counts and output-pointer displacements. */
asm void subtract_immediate_aliases(void) {
    nofralloc
    subi r6, r6, 1
    subi r5, r5, 4
    subi r3, r4, -32767
    subi r7, r8, 32768
    subis r9, r10, 4
    subis r11, r12, -1
    blr
}
