// The leaf if/else TWO-EXIT form: when the return value is ALREADY in the result register (`return a`,
// the condition variable, in r3) and the store arms leave it intact, mwcc does NOT build a shared join —
// each arm stores then returns directly: `cmpwi; b<!c> else; <then-store>; blr; else: <else-store>; blr`.
// This is the two-EXIT diamond (like a void if/else, plus the in-place r3 return), distinct from the JOIN
// form (canary 1133/1134) that a materialized return uses. Local branch labels do not advance @N.
// (fire 621 — general #21, the return-value-already-in-r3 case)
int te_gt(int a, int* p)          { if (a > 0) { *p = 1; } else { *p = 2; } return a; }  // ble; li r0,1; stw; blr; li r0,2; stw; blr
int te_lt(int a, int* p)          { if (a < 0) { *p = 1; } else { *p = 2; } return a; }  // bge; ...
int te_selfstore(int a, int* p)   { if (a > 0) { *p = a; } else { *p = 2; } return a; }  // then-arm stores the return var
int te_diff(int a, int* p, int* q){ if (a > 0) { *p = 1; } else { *q = 2; } return a; }  // distinct pointers
