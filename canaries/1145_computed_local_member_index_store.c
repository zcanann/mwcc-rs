// Extends the register-kept computed-local slice (canaries 1141-1144) to MEMBER and constant-INDEX
// store targets: `int t = <single-op>; p->y = t; return t;` -> `addi r3,r3,1; stw r3,4(r4); blr`,
// and `p[3] = t` -> `stw r3,12(r4)`. Like a bare deref, a member (`p->field`) or a CONSTANT subscript
// (`p[3]`) is a parameter plus a FIXED displacement, so the address never touches the value's register
// and t survives to the return (and works in the r0-void case too). A VARIABLE subscript would compute
// its offset into a register and defers. Targets may be freely mixed with derefs and (r3-return only)
// globals. (fire 634)
struct S { int x; int y; };
int cli_member(int a, struct S* p)             { int t = a + 1; p->y = t; return t; }           // stw r3,4(r4)
int cli_index(int a, int* p)                   { int t = a + 1; p[3] = t; return t; }            // stw r3,12(r4)
int cli_member_postop(int a, struct S* p)      { int t = a * 2; p->x = t; return t + 1; }        // mullw; stw r3,0(r4); addi r3,r3,1
int cli_mixed(int a, int* p, struct S* q)      { int t = a + 1; *p = t; q->y = t; return t; }    // stw r3,0(r4); stw r3,4(r5)
void cli_void_member(int a, struct S* p, struct S* q) { int t = a + 1; p->y = t; q->x = t; }      // addi r0; stw r0,4(r4); stw r0,0(r5)
