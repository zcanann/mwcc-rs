// flags: -sym on -Cpp_exceptions off
/* Unused C inline asm helpers affect symbols and debug anonymous numbering. */
static inline void unused_static_helper(void) {
    asm {
        li r3, 4
    }
}
inline void unused_global_helper(void) {
    asm {
        li r4, 5
    }
}
int inline_asm_debug_boundary(void) {
    return 1;
}
