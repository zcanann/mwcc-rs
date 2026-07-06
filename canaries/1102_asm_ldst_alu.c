asm void asm_ldst_alu(void){ nofralloc
 stwu r1, -0x10(r1)
 stw r31, 8(r1)
 lwz r0, 0(r3)
 lbz r5, 3(r3)
 lhz r6, 4(r3)
 add r4, r0, r5
 subf r6, r4, r5
 subfc r7, r0, r5
 adde r8, r0, r5
 or r9, r0, r4
 and r10, r0, r4
 xor r11, r0, r4
 nor r12, r0, r4
 slw r5, r0, r4
 srw r6, r0, r4
 neg r7, r0
 cntlzw r8, r0
 addi r3, r3, 4
 subfic r9, r0, 0
 ori r10, r0, 0x8000
 oris r11, r0, 0x1234
 lis r12, 0x1234
 addis r0, r5, 0x10
 stb r9, 5(r4)
 sth r10, 6(r4)
 lfd f1, 0x20(r1)
 lfs f3, 0x18(r1)
 stfd f2, 0x28(r1)
 stfs f4, 0x1c(r1)
 lwz r31, 8(r1)
 addi r1, r1, 0x10
 blr }
