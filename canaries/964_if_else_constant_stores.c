// A whole-body `if (c) { <constant run> } else { <constant run> }` (both arms two-or-more constant
// stores) lowers to the branch-over form: `cmpwi; beq else; <then run>; blr; else: <else run>; blr`.
// Each arm reuses mwcc's batched constant materialization (r(N+1)..r3,r0), independently — the arms
// are on separate branches, so both may use the same registers.
//   if(a){g=1;h=2;}else{g=3;h=4;} -> cmpwi;beq L; li r3,1;li r0,2;stw r3,g;stw r0,h;blr;
//                                    L: li r3,3;li r0,4;stw r3,g;stw r0,h;blr
int g, h, k, m;

void same_targets(int a)      { if (a)     { g = 1; h = 2; } else { g = 3; h = 4; } }
void diff_targets(int a)      { if (a)     { g = 1; h = 2; } else { k = 3; m = 4; } }
void three_each(int a)        { if (a)     { g = 1; h = 2; k = 3; } else { g = 4; h = 5; k = 6; } }
void compare_cond(int a, int b) { if (a > b) { g = 1; h = 2; } else { g = 3; h = 4; } }
