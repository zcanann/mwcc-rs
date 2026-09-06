/* Matrix/vector assembly instruction families from Dolphin mtxvec.c. */
asm void paired_matrix_lanes(void) {
    nofralloc
    ps_merge00 f0, f13, f12
    ps_merge01 f2, f11, f10
    ps_merge10 f31, f0, f30
    ps_merge11 f1, f13, f12
    ps_muls1 f8, f1, f6
    ps_madds0 f12, f2, f7, f8
    ps_madds1 f8, f1, f6, f8
    psq_lu f7, 8(r4), 1, 0
    psq_lu f31, -2048(r3), 0, 7
    psq_stu f12, 4(r5), 0, 0
    psq_stu f0, 2047(r31), 1, 7
    blr
}
