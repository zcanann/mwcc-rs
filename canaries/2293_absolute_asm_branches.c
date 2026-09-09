/* I-form AA/LK fields, discarded low bits and signed address limits. */
extern void destination(void);
asm void jump_low(void) { nofralloc
    ba 0x60
}
asm void jump_unaligned(void) { nofralloc
    ba 0x63
}
asm void jump_high(void) { nofralloc
    ba 0x1ffffff
}
asm void jump_negative(void) { nofralloc
    ba 0xfe000000
}
asm void call_low(void) { nofralloc
    bla 0x60
}
asm void call_unaligned(void) { nofralloc
    bla 0x61
}
asm void call_high(void) { nofralloc
    bla 0x1fffffc
}
asm void call_negative(void) { nofralloc
    bla 0xffffffff
}
asm void call_expression(void) { nofralloc
    bla (0x40+0x20)
}
asm void call_wrapped(void) { nofralloc
    bla 0x100000060ULL
}
asm void jump_symbol(void) { nofralloc
    ba destination
}
asm void call_symbol(void) { nofralloc
    bla destination
}
