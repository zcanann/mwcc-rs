// Select with one NON-ZERO constant arm and one register-leaf arm, as a guarded return:
// `if (c) return 5; return a;` (== `c ? 5 : a`). mwcc materializes the constant into the
// result and conditional-returns ONLY when the leaf is unrelated and not in the result
// register (`li r3,C; bnelr; mr r3,x`, x in r4). When the leaf already lives in the result
// register (a in r3), that form would `li r3,C` over the leaf and then self-move-coalesce
// the `mr r3,r3` away — a SILENT MISCOMPILE (the c==0 path returns C instead of the leaf).
// So mwcc stages the constant in r0, conditionally moves the leaf over it, then `mr r3,r0`:
//
//     cmpwi r4,0 ; li r0,5 ; bne L ; mr r0,r3 ; L: mr r3,r0 ; blr        (if(c) return 5; return a)
//     cmpwi r4,0 ; li r0,5 ; beq L ; mr r0,r3 ; L: mr r3,r0 ; blr        (if(c) return a; return 5)
//
// The same staging fires when the leaf is a condition operand (`(a>b) ? 7 : b`).
int gi;
int const_then_leaf_r3(int a, int c) { if (c) return 5; return a; }   // a in r3 -> r0 staging
int leaf_r3_then_const(int a, int c) { if (c) return a; return 5; }   // a in r3 -> r0 staging
int const_then_leaf_r4(int c, int x) { if (c) return 5; return x; }   // x in r4 -> li r3,5; bnelr; mr
int leaf_r4_then_const(int c, int x) { if (c) return x; return 5; }   // x in r4 -> li r3,5; beqlr; mr
