// flags: -sym on
/* Void asm bodies carry instruction rows rather than empty-C-body rows. */
asm void written_void(void) {
    nofralloc
    sync
    isync
    blr
}
asm void implicit_void(void) {
    sync
    isync
}
