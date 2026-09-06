/* GC/1.3 lets a negative displacement overwrite the W/I fields. */
asm void paired_negative_displacements(void) {
    nofralloc
    psq_l f1, -1(r3), 0, 0
    psq_l f2, -16(r4), 1, 2
    psq_st f3, -2048(r5), 0, 3
    psq_lu f4, -8(r6), 0, 0
    psq_stu f5, -32(r7), 0, 1
    psq_l f6, 0(r8), 0, 0
    psq_stu f7, 2047(r9), 0, 2
    blr
}
