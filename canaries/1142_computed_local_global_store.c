// Extends the register-kept computed-local slice (canary 1141) to DIRECT GLOBAL store targets:
// `int t = <single-op>; gg = t; [hh = t;] [*p = t;] return t;`. t stays in the result register r3;
// each global store writes it via the global's SDA21/ADDR16 addressing (base r0/r2/r13 — never r3),
// so t survives to the return: `addi r3,r3,1; stw r3,gg; [stw r3,hh;] blr`. The store target may be a
// direct global or a bare parameter deref, freely mixed. Global addressing differs by compiler version
// (SDA21 vs ADDR16), so this exercises the store emission across the version matrix. (fire 630)
int gg;
int hh;
int clg_one(int a)          { int t = a + 1; gg = t; return t; }              // stw r3,gg
int clg_two(int a)          { int t = a + 1; gg = t; hh = t; return t; }       // stw r3,gg; stw r3,hh
int clg_mixed(int a, int* p){ int t = a + 1; gg = t; *p = t; return t; }       // stw r3,gg; stw r3,0(r4)
int clg_mul(int a, int b)   { int t = a * b; gg = t; return t; }               // mullw r3,r3,r4; stw r3,gg
