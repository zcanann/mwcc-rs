// The VOID multi-store case of the register-kept computed-local slice (canaries 1141-1143): a computed
// local stored to TWO+ pointers with no return. Nothing is returned, so mwcc keeps the value in the
// scratch r0 (not r3) and stores it to each pointer: `int t=a+1; *p=t; *q=t;` -> `addi r0,r3,1;
// stw r0,0(r4); stw r0,0(r5); blr`. The SINGLE-store void is the computed-store-fill path (left to it),
// so this handler requires 2+ stores; only parameter derefs (a global's ADDR16 address temp would be
// r0, clobbering the kept value). (fire 632)
void vms_two(int a, int* p, int* q)          { int t = a + 1; *p = t; *q = t; }          // addi r0,r3,1; stw r0,0(r4); stw r0,0(r5)
void vms_mul(int a, int b, int* p, int* q)   { int t = a * b; *p = t; *q = t; }          // mullw r0,r3,r4; stw r0,0(r5); stw r0,0(r6)
void vms_three(int a, int* p, int* q, int* r){ int t = a + 1; *p = t; *q = t; *r = t; }  // addi r0; three stores
