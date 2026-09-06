/* A pointer to float uses a GPR; a floating value uses the separate FPR cursor. */
asm void float_pointer_arguments(register double value, register float* destination,
                                 const register float* source) {
    nofralloc
    lfs f0, 0(source)
    fsub f0, f0, f1
    stfs f0, 0(destination)
    blr
}
