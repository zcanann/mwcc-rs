// builds: 1.1 1.1p1 1.2.5 1.2.5n 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7

static inline void unused_asm_helper(void) {
    asm {
        li r3, 4
    }
}

int retained_inline_asm_symbol_boundary(void) {
    return 1;
}
