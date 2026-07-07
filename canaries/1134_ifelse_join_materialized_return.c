// The leaf if/else JOIN form (canary 1133) with a MATERIALIZED return continuation — the merge point
// evaluates the return value into r3: a parameter not already in r3 (`mr r3,rN`), or a `param ± const`
// (`addi`). `if(a>0){*p=1;}else{*p=2;} return b;` -> `cmpwi;ble;li r0,1;stw;b;li r0,2;stw; mr r3,r4; blr`.
// A return value ALREADY in r3 (`return a`, the condition var) uses the two-EXIT form (each arm blr's)
// and still defers here. (fire 621 — general #21, extends 1133 from constant to any materialized return)
int jm_reg(int a, int b, int* p)   { if (a > 0) { *p = 1; } else { *p = 2; } return b; }    // join: mr r3,r4
int jm_addi(int a, int* p)         { if (a > 0) { *p = 1; } else { *p = 2; } return a + 1; }// join: addi r3,r3,1
int jm_subi(int a, int* p)         { if (a > 0) { *p = 1; } else { *p = 2; } return a - 1; }// join: addi r3,r3,-1
int jm_regstore(int a, int b, int* p){ if (a > 0) { *p = b; } else { *p = 2; } return b; }  // register store arm + mr
