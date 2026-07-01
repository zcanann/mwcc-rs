// A VOID early-return guard over a single store: `if (a) return; *p = 5;` is a
// conditional RETURN (bnelr -- the void exit needs no value) followed by the plain
// standalone store body: `cmpwi r3,0; bnelr; li r0,5; stw r0,0(r4); blr`. Bare
// `return;` guards previously failed to parse (a GuardedReturn requires a value); they
// now enter the ordered statement list as `If { then: [Return(None)] }`.
struct S { int a; int x; };
int gi;

void store_const(int a, int *p)          { if (a) return; *p = 5; }
void store_reg(int a, int *p, int x)     { if (a) return; *p = x; }
void store_computed(int a, int *p, int x){ if (a) return; *p = x + 1; }
void store_member(int a, struct S *p)    { if (a) return; p->x = 9; }
void store_global(int a)                 { if (a) return; gi = 5; }
void store_indexed(int a, int *p)        { if (a > 3) return; p[2] = 7; }
