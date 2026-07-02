// An OR guard whose FALL-THROUGH value already sits in the result register folds the
// last term into a conditional return -- `if (s < 1 || s > 6) return -1; return s;`
// emits `cmpwi s,1; blt TAKEN; cmpwi s,6; blelr; TAKEN: li r3,-1; blr` (the false side
// of the last term returns s directly; no fall block exists). Previously the last term
// branched to an EMPTY fall block holding a bare blr -- one extra branch, a miscompile.
int range_check(int s)  { if (s < 1 || s > 6) return -1; return s; }
int eq_or_eq(int a)     { if (a == 2 || a == 7) return 9; return a; }
