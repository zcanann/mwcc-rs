// flags: -sym on -Cpp_exceptions off -pragma "cats off"
/* The SDK stub's unused helpers exist only to populate code address tables. */
static inline void unused_local_helper(void) {
    asm {
        li r3, 4
    }
}
inline void unused_global_helper(void) {
    asm {
        li r4, 5
    }
}
unsigned char stub_present(void) {
    return 0;
}
