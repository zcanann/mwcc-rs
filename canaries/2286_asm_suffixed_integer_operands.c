/* SDK asm immediates retain integer literal bits regardless of C suffix. */
asm int unsigned_immediate(void) { nofralloc
    li r3, 2U
    blr
}
asm int negative_unsigned(void) { nofralloc
    li r3, -2U
    blr
}
asm int folded_unsigned(void) { nofralloc
    li r3, (2U | 4U)
    blr
}
asm int wide_constant(void) { nofralloc
    li r3, (1ULL << 4)
    blr
}
asm int numeric_high(void) { nofralloc
    lis r3, 0x12345678U@h
    ori r3, r3, 0x12345678U@l
    blr
}
asm int wide_low(void) { nofralloc
    li r3, 0x100000001ULL@l
    blr
}
asm int complemented(void) { nofralloc
    li r3, ~0U
    blr
}
asm int suffixed_displacement(void) { nofralloc
    lwz r3, 4U(r4)
    blr
}
asm int folded_displacement(void) { nofralloc
    lwz r3, (4U + 4U)(r4)
    blr
}
asm int truncated_wide(void) { nofralloc
    li r3, 0x100000000ULL
    blr
}
asm int signed_word_bits(void) { nofralloc
    li r3, 0xffffffffU
    blr
}
asm int signed_asm_shift(void) { nofralloc
    li r3, (0xffffffffU >> 31)
    blr
}
asm int oversized_asm_shift(void) { nofralloc
    li r3, (1ULL << 40) >> 39
    blr
}
asm int zero_asm_divisor(void) { nofralloc
    li r3, (5U / 0U) + (-5U % 0U)
    blr
}
