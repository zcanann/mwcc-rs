// A guard chain whose TAIL folds to a branchless sign-mask no longer needs cross-guard CR reuse.
// `if(a>0)return 1; if(a<0)return -1; return 0;` (the sgn pattern, and its if/else-if/else form):
// mwcc emits ONE compare for the first guard and folds `if(a<0)return -1; return 0;` into the
// sign-mask `srawi r3,r3,31` (no second compare). The previous defer ("consecutive guards sharing
// a compare") was too eager — it assumed the second guard emits a redundant compare, but a
// sign-mask tail emits none. body.rs emit_guard_sequence now skips the defer for the (2nd-to-last,
// last) pair when the last guard folds to sign_mask_select. Genuine CR-reuse (compare-based tails
// like `if(a<0)..; if(a==0)..`) still defers. First task #21 increment reaching real-file patterns.
int sgn_chain(int a) { if (a > 0) return 1; if (a < 0) return -1; return 0; }
int sgn_elif(int a)  { if (a > 0) return 1; else if (a < 0) return -1; else return 0; }
