// flags: -sym on
/* Implicit returns are unmarked, except for a line-zero row in GC/1.3. */
asm int implicit_return(void) {
    li r3, 7
}
asm int written_return(void) {
    li r3, 9
    blr
}
