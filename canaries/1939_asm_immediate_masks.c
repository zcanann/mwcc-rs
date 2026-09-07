// flags: -Cpp_exceptions off -pragma "cats off"
// GXInit's write-gather helper uses andi. with no whitespace after the dot.
asm unsigned compact_mask(unsigned value) {
    nofralloc
    andi.r3, r3, 1
    blr
}
asm unsigned low_mask(unsigned value) {
    nofralloc
    andi. r3, r3, 0x8001
    blr
}
asm unsigned high_mask(unsigned value) {
    nofralloc
    andis. r3, r3, 0x8001
    blr
}
asm unsigned low_flags(unsigned value) {
    nofralloc
    andi. r0, r3, 0xffff
    mfcr r3
    blr
}
asm unsigned high_flags(unsigned value) {
    nofralloc
    andis. r0, r3, 0xffff
    mfcr r3
    blr
}
asm unsigned zero_mask(unsigned value) {
    nofralloc
    andis. r3, r3, 0
    blr
}
asm unsigned low_branch(unsigned value) {
    nofralloc
    andi. r0, r3, 0x8000
    beq clear
    li r3, 1
    blr
clear:
    li r3, 0
    blr
}
asm unsigned high_branch(unsigned value) {
    nofralloc
    andis. r0, r3, 0x8000
    blt negative
    li r3, 0
    blr
negative:
    li r3, 1
    blr
}
