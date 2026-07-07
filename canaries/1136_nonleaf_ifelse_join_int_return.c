// The NON-LEAF if/else JOIN form with a constant int-return continuation — the void non-leaf join
// (a call in a branch) extended to return a small constant. mwcc materializes the return BETWEEN the
// LR reload and the mtlr at the join: `stwu;mflr;cmpwi;stw r0,20;ble else; li r0,1;stw;b join;
// else: bl g; join: lwz r0,20; li r3,C; mtlr; addi; blr`. The reload-hoist pass bails on the join's
// branches, so the epilogue is emitted explicitly (LR reload, then the return, then mtlr). Distinct
// from the LEAF join (canaries 1133/1134, no frame). (fire 622 — general #21)
void sink(void);
int nlj_then(int a, int* p) { if (a > 0) { *p = 1; } else { sink(); } return 0; }  // then-store / else-call
int nlj_else(int a, int* p) { if (a > 0) { sink(); } else { *p = 1; } return 0; }  // call / store
int nlj_ret5(int a, int* p) { if (a > 0) { *p = 1; } else { sink(); } return 5; }  // return 5
void nlj_void(int a, int* p){ if (a > 0) { *p = 1; } else { sink(); } }            // void form still matches
