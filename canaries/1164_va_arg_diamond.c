// The __va_arg g<max DIAMOND at parameter scale — the register-counter then-arm against the
// fire-672 ALIGN else-arm, joining `addr` in the store-base register. Measured (list r3, size r4,
// reg r5, g r6, max r7): the then-arm reclaims size's register for the member load (size is dead
// there), list's for the scaled counter (dead after the load), and reg's for the join value; the
// store value folds the mutation (`addi r0,r6,C1+C2`); the else-arm is exactly the fire-672
// schedule with g's dead register (first dead volatile) as the value register:
//   cmpw r6,r7; bge ELSE; lwz r4,4(r3); addi r0,r6,2; addi r3,r6,1; slwi r3,r3,2; stb r0,0(r5);
//   add r5,r4,r3; b JOIN; ELSE: li r6,8; addi r0,r4,-1; stb r6,0(r5); not r6,r0; lwz r0,4(r3);
//   add r5,r4,r0; addi r0,r5,-1; and r5,r6,r0; add r0,r5,r4; stw r0,4(r3); JOIN: mr r3,r5; blr
// The diamond advances the anonymous-@N counter by 4 (else target + join, 2 each) — calibrated
// with a byte-exact full-object probe against a trailing @N-consuming function (fire 673).
// Shuffled parameter arrangements defer (the reclaim schedule is measured for this order only).
typedef struct VL5 { int gpr; char* area; } VL5;
char* vad(VL5* list, int size, char* reg, int g, int max) {
    char* addr;
    if (g < max) {
        g += 1;
        addr = list->area + g * 4;
        *reg = g + 1;
    } else {
        *reg = 8;
        addr = list->area;
        addr = (char*)(((unsigned long)addr + (size - 1)) & ~(size - 1));
        list->area = addr + size;
    }
    return addr;
}
char* vad2(VL5* list, int size, char* reg, int g, int max) {
    char* addr;
    if (g < max) {
        g += 2;
        addr = list->area + g * 8;
        *reg = g + 2;
    } else {
        *reg = 8;
        addr = list->area;
        addr = (char*)(((unsigned long)addr + (size - 1)) & ~(size - 1));
        list->area = addr + size;
    }
    return addr;
}
void* vadn(void* p, unsigned char t) { if (t == 0) { p = *(void**)p; } return p; }
