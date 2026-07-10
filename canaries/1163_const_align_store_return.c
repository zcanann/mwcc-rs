// The __va_arg type==4 block at whole-function scale — the CONSTANT-size ALIGN with a member
// store-back: `addr = (char*)(((unsigned long)list->area + 15) & ~15); list->area = addr + 16;
// return addr;`. Measured: the member load lands in the first free volatile as addr's home; the
// const mask folds to clrrwi; the +stride fills the scratch for the store-back:
//   lwz r4,4(r3); addi r0,r4,15; clrrwi r4,r0,4; addi r0,r4,16; stw r0,4(r3); mr r3,r4; blr
// The parser const-folds the real-file spelling `+ (16-1) & ~(16-1)` to the same shape (fire 673).
typedef struct VL4 { int gpr; char* area; } VL4;
char* ca16(VL4* list) { char* addr; addr = (char*)(((unsigned long)list->area + 15) & ~15); list->area = addr + 16; return addr; }
char* ca8(VL4* list) { char* addr; addr = (char*)(((unsigned long)list->area + 7) & ~7); list->area = addr + 8; return addr; }
char* caspell(VL4* list) { char* addr; addr = (char*)(((unsigned long)list->area + (16 - 1)) & ~(16 - 1)); list->area = addr + 0x10; return addr; }
