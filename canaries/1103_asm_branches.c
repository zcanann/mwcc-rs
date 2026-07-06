asm void asm_branches(void){ nofralloc
 mtctr r4
 cmpwi r3, 0
 beq early_ret
 cmplwi r3, 8
 bne skip
 addi r3, r3, 1
skip:
 add r6, r3, r5
loop:
 lwz r7, 0(r6)
 addi r6, r6, 4
 cmpw r7, r5
 blt take_lt
 bgt take_gt
 bge take_ge
 ble take_le
 bdnz loop
 b done
take_lt:
take_gt:
take_ge:
take_le:
 or r3, r7, r6
done:
 blr
early_ret:
 blr }
