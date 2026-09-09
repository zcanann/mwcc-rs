extern void destination(void);
asm void framed_call(void) {
    fralloc
    li r3, 7
    bla destination
    frfree
    blr
}
unsigned retain_input(unsigned x) {
    asm { bla destination }
    return x + 1;
}
unsigned retain_computed(unsigned x, unsigned y) {
    unsigned value = x + y;
    asm { bla destination }
    return value + 3;
}
