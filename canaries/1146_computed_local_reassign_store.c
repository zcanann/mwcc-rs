// Extends the register-kept computed-local slice (canaries 1141-1145) with leading in-place
// REASSIGNMENTS of the local before the stores: `int t = a+1; t = t+b; *p = t; return t;` ->
// `addi r3,r3,1; add r3,r3,r4; stw r3,0(r5); blr`. The value stays in one register (r3, or r0 when
// void); each `t = t OP <param>` mutates it in place (add/subf/mullw/and/or/xor/shift). Only a REGISTER
// right operand — a constant step folds into the init in mwcc (`t=a+1; t=t+5` -> `addi r3,r3,6`), so
// those defer — and it must not be the param sitting in the value's own register (which t overwrote).
// This is the "local reassignment mixed with stores" cluster's integer core. (fire 635)
int clr_add(int a, int b, int* p)            { int t = a + 1; t = t + b; *p = t; return t; }             // add r3,r3,r4
int clr_two(int a, int b, int c, int* p)     { int t = a + 1; t = t + b; t = t + c; *p = t; return t; }  // add; add
int clr_sub(int a, int b, int* p)            { int t = a + 1; t = t - b; *p = t; return t; }             // subf r3,r4,r3
int clr_mul(int a, int b, int* p)            { int t = a + 1; t = t * b; *p = t; return t; }             // mullw r3,r3,r4
void clr_void(int a, int b, int* p, int* q)  { int t = a + 1; t = t + b; *p = t; *q = t; }               // add r0,r0,r4; two stores
