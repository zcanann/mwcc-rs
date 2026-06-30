// A multi-guard sequence with CONSTANT early-return values — `if (a < 0) return -1; if (a > 10)
// return 1; return a;` — the common chain of error/bounds checks. Each non-last constant guard
// is `cmpwi; bge skip; li result, c; blr; skip:`; the last guard folds into the fall-through as a
// select. The guards must test DISTINCT comparisons: mwcc reuses one `cmpwi` across consecutive
// guards comparing the same operand to the same constant (`if (a<0)...; if (a==0)...` shares
// `cmpwi r3,0`), which is not modeled and defers rather than emitting a redundant compare.
int two_guard_leaf(int a)               { if (a < 0) return -1; if (a > 10) return 1; return a; }
int two_guard_arith(int a)              { if (a < 0) return -1; if (a > 10) return 1; return a + 5; }
int two_guard_vars(int a, int b, int c) { if (a < 0) return -1; if (b < 0) return -2; return c; }
