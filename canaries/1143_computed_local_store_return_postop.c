// Extends the register-kept computed-local slice (canaries 1141/1142) so the return may apply ONE
// additive constant post-op to the kept local, in place: `int t = <single-op>; *p = t; return t + 1;`
// -> `addi r3,r3,1; stw r3,0(r4); addi r3,r3,1; blr`. t stays in r3 across the store; the post-op
// mutates r3 just before blr. Only `+`/`-` with an int16 constant keep t in r3 for the in-place addi;
// `* & << >>` instead put t in r0 and compute the result into r3 (`slwi r3,r0,2`) — an allocation
// choice not modeled here — so those, and non-constant post-ops, defer. (fire 631)
int gg;
int clsp_add(int a, int* p)        { int t = a + 1; *p = t; return t + 1; }   // addi r3,r3,1; stw; addi r3,r3,1
int clsp_sub(int a, int* p)        { int t = a + 1; *p = t; return t - 2; }   // addi r3,r3,1; stw; addi r3,r3,-2
int clsp_addv(int a, int b, int* p){ int t = a + b; *p = t; return t + 5; }   // add r3,r3,r4; stw; addi r3,r3,5
int clsp_global(int a)             { int t = a + 1; gg = t; return t + 1; }   // addi r3,r3,1; stw r3,gg; addi r3,r3,1
