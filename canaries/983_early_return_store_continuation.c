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
