// An early-return guard over a STORE continuation. A constant store value materializes
// into r0 with the return value scheduled BETWEEN the `li` and the store:
// `cmpwi; beq CONT; li r3,-1; blr; CONT: li r0,5; li r3,0; stw r0,0(r4); blr`
// (or `mr r3,x` for a register return). A register-valued store needs no
// materialization: the store comes first, then the return move. (Previously the
// sequential emission put the store before the return `li` -- a byte-DIFF.)
int store_const_ret_const(int a, int *p)         { if (a) return -1; *p = 5; return 0; }
int store_const_ret_reg(int a, int *p, int x)    { if (a) return -1; *p = 5; return x; }
int store_const_indexed(int a, int *p)           { if (a) return -1; p[2] = 5; return 0; }
int store_reg_ret_const(int a, int *p, int x)    { if (a) return -1; *p = x; return 0; }

// A p->member target and a COMPUTED (two-leaf) store value take the same schedule --
// the value materializes into r0 (`addi r0,r5,1` / `add r0,r5,r6`), the return value
// between it and the store.
struct S { int a; int x; };
int store_member_const(int a, struct S *p)            { if (a) return -1; p->x = 5; return 0; }
int store_member_computed(int a, struct S *p, int v)  { if (a) return -1; p->x = v + 2; return 0; }
int store_computed(int a, int *p, int x)              { if (a) return -1; *p = x + 1; return 0; }
int store_computed_two_reg(int a, int *p, int x, int y) { if (a) return -1; *p = x + y; return 0; }
int store_computed_indexed(int a, int *p, int x)      { if (a) return -1; p[3] = x * 2; return 0; }
