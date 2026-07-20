// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -inline deferred -str reuse,pool,readonly

#pragma cplusplus off
static inline void setup_quantizers(void) {
    asm {
        li r3, 0x0004
        oris r3, r3, 0x0004
        mtspr GQR2, r3
        li r3, 0x0005
        oris r3, r3, 0x0005
        mtspr GQR3, r3
        li r3, 0x0006
        oris r3, r3, 0x0006
        mtspr GQR4, r3
        li r3, 0x0007
        oris r3, r3, 0x0007
        mtspr GQR5, r3
    }
}
#pragma cplusplus reset

float skipped_static_inline_asm_probe() {
    return 1.0f;
}
