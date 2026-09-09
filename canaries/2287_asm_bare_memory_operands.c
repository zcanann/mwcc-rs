/* SDK symbol operands leave SDA base selection to the linker. */
int word;
float single;
double real;
asm int load_word(void) { nofralloc
    lwz r3, word
    blr
}
asm void store_word(void) { nofralloc
    stw r3, word
    blr
}
asm float load_single(void) { nofralloc
    lfs f1, single
    blr
}
asm double load_double(void) { nofralloc
    lfd f1, real
    blr
}
asm void store_single(void) { nofralloc
    stfs f1, single
    blr
}
asm void store_double(void) { nofralloc
    stfd f1, real
    blr
}
asm int low_memory(void) { nofralloc
    lwz r3, 0xD4U
    blr
}
