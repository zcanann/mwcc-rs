// The __va_arg ELSE-arm — the variable-size ALIGN idiom with a member store-back:
// `*reg = 8; addr = list->area; addr = (char*)(((unsigned long)addr + (size-1)) & ~(size-1));
// list->area = addr + size; return addr;`. Measured (register reclaim throughout): the store VALUE
// materializes in the first free volatile; `size-1` fills the slot before the store; `not` reclaims
// the value register; the area load, sum, and mask chain reclaim the store-base register as addr's home:
//   li r6,8; addi r0,size,-1; stb r6,0(reg); not r6,r0; lwz r0,4(list); add r5,size,r0;
//   addi r0,r5,-1; and r5,r6,r0; add r0,r5,size; stw r0,4(list); mr r3,r5; blr
// (fire 672 — the last major __va_arg arm; the wrapping diamond and composition remain)
typedef struct VL3 { int gpr; char* area; } VL3;
char* asa(VL3* list, int size, char* reg) { char* addr; *reg = 8; addr = list->area; addr = (char*)(((unsigned long)addr + (size - 1)) & ~(size - 1)); list->area = addr + size; return addr; }
