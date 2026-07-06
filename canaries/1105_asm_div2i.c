asm void __div2i(void) {
	nofralloc
	stwu    r1,-16(r1)
	rlwinm. r9,r3,0,0,0
	beq     cr0,positive1
	subfic  r4,r4,0
	subfze  r3,r3
positive1:
	stw     r9,8(r1)
	rlwinm. r10,r5,0,0,0
	beq     cr0,positive2
	subfic  r6,r6,0
	subfze  r5,r5
positive2:
	stw     r10,12(r1)
	cmpwi   cr0,r3,0
	cntlzw  r0,r3
	cntlzw  r9,r4
	bne     cr0,lab1
	addi    r0,r9,32
lab1:
	cmpwi   cr0,r5,0
	cntlzw  r9,r5
	cntlzw  r10,r6
	bne     cr0,lab2
	addi    r9,r10,32
lab2:
	cmpw    cr0,r0,r9
	subfic  r10,r0,64
	bgt     cr0,lab9
	addi    r9,r9,1
	subfic  r9,r9,64
	add     r0,r0,r9
	subf    r9,r9,r10
	mtctr   r9
	cmpwi   cr0,r9,32
	addi    r7,r9,-32
	blt     cr0,lab3
	srw     r8,r3,r7
	li      r7,0
	b       lab4
lab3:
	srw     r8,r4,r9
	subfic  r7,r9,32
	slw     r7,r3,r7
	or      r8,r8,r7
	srw     r7,r3,r9
lab4:
	cmpwi   cr0,r0,32
	addic   r9,r0,-32
	blt     cr0,lab5
	slw     r3,r4,r9
	li      r4,0
	b       lab6
lab5:
	slw     r3,r3,r0
	subfic  r9,r0,32
	srw     r9,r4,r9
	or      r3,r3,r9
	slw     r4,r4,r0
lab6:
	li      r10,-1
	addic   r7,r7,0
lab7:
	adde    r4,r4,r4
	adde    r3,r3,r3
	adde    r8,r8,r8
	adde    r7,r7,r7
	subfc   r0,r6,r8
	subfe.  r9,r5,r7
	blt     cr0,lab8
	mr      r8,r0
	mr      r7,r9
	addic   r0,r10,1
lab8:
	bdnz    lab7
	adde    r4,r4,r4
	adde    r3,r3,r3
	lwz     r9,8(r1)
	lwz     r10,12(r1)
	xor.    r7,r9,r10
	beq     cr0,no_adjust
	cmpwi   cr0,r9,0
	subfic  r4,r4,0
	subfze  r3,r3

no_adjust:
	b       func_end

lab9:
	li      r4,0
	li      r3,0
func_end:
	addi    r1,r1,16
	blr
}
