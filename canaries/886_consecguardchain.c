// Guard chains whose TAIL folds to a consecutive-constant branchless sign select also no longer
// falsely defer. Extends canary 885 (sign-mask tails) to the `(x REL 0) ? c1 : c2` branchless forms
// (<0/>0/>=0): `if(a<0)return 1; if(a>0)return 2; return 3;` -> mwcc emits ONE compare for the
// first guard and folds `if(a>0)return 2; return 3;` into `neg;andc;srawi;addi` (no second compare).
// body.rs emit_guard_sequence skips the shared-key defer when the last guard folds branchlessly
// (control_flow::select_folds_branchless = sign_mask_select OR sign_consecutive_select). Compare-
// based tails (==0/!=0/<=0/variable) are NOT branchless and keep deferring (no DIFF shipped).
int cls_lt_gt(int a) { if (a < 0)  return 1; if (a > 0) return 2; return 3; }
int cls_eq_gt(int a) { if (a == 0) return 1; if (a > 0) return 2; return 3; }
