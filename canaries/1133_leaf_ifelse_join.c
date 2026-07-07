// A LEAF if/else whose two arms both STORE and then MERGE at a join before a shared constant return
// — mwcc's two-branch join form: `cmpwi; b<!c> else; <then-stores>; b join; else: <else-stores>;
// join: li r3,C; blr`. Distinct from the two-EXIT diamond (each arm blr's) that a void if/else uses:
// here a `return <const>` continuation forces the merge. No call -> leaf, no frame/LR-save. The arms may
// store to the same or different pointers, constant or register values; the return is a small constant.
// A register return (`return a`) needs a different tail and still defers. (fire 620 — general #21 slice)
int j_same(int a, int* p)          { if (a > 0) { *p = 1; } else { *p = 2; } return 0; }  // cmpwi;ble;li r0,1;stw;b;li r0,2;stw;li r3,0;blr
int j_diff(int a, int* p, int* q)  { if (a > 0) { *p = 1; } else { *q = 2; } return 0; }
int j_ret5(int a, int* p)          { if (a > 0) { *p = 1; } else { *p = 2; } return 5; }
int j_regval(int a, int* p)        { if (a > 0) { *p = a; } else { *p = 2; } return 0; }  // then-arm stores a register value
