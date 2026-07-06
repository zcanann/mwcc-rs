asm void asm_autoframe(void){
 stwu r1, -0x10(r1)
 clrrwi. r5, r3, 31
 beq skip
 subfic r4, r4, 0
 subfze r3, r3
skip:
 stw r3, 8(r1)
 lwz r3, 8(r1)
 addi r1, r1, 16
 blr }
