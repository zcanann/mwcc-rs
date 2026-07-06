asm void asm_branch_hints(void){ nofralloc
 cmpwi r3, 0
 ble+ fwd
 addi r3, r3, 1
fwd:
 beq- fwd2
 addi r3, r3, 2
fwd2:
back:
 addi r4, r4, -1
 cmpwi r4, 0
 beq- back
 bne+ back
 blr }
