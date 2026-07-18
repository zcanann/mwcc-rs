asm void asm_leaf_explicit_return(void) {
    blr
}

asm void asm_leaf_fallthrough(void) {
    mr r3, r4
}

asm void asm_stack_explicit_return(void) {
    stwu r1, -16(r1)
    addi r1, r1, 16
    blr
}

asm void asm_stack_fallthrough(void) {
    stwu r1, -16(r1)
    addi r1, r1, 16
}
