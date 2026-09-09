extern void destination(void);
asm unsigned zero_low(void) { nofralloc
    addi r3, 0, 0x7654
    blr
}
asm unsigned zero_high(void) { nofralloc
    addis r3, 0, 0x89ab
    blr
}
asm void *zero_symbol(void) { nofralloc
    addis r3, 0, destination@ha
    addi r3, r3, destination@l
    blr
}
