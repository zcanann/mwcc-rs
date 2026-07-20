// MWCC accepts the asm function qualifier on either side of the return type. Dolphin's reset
// implementation uses this infix form, including a static storage qualifier and named register.
// builds: GC/1.2.5
static void asm infix_asm_qualifier(register int code) {
    nofralloc
    mr r4, code
    blr
}
