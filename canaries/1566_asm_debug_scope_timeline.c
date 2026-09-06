// flags: -sym on -Cpp_exceptions off
/* GC/2.7 assembly bodies do not consume the ordinary four scope ordinals. */
asm void first_assembly(void) {
    nofralloc
    li r3, 1
    blr
}
asm void second_assembly(void) {
    nofralloc
    li r3, 2
    blr
}
int ordinary_body(void) {
    return 3;
}
