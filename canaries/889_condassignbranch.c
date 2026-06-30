// `T y = INIT; if (c) y = NEW; return y;` (if with NO else), CONSTANT arms. mwcc lowers this
// conditional ASSIGN as an early-return BRANCH form — distinct from the select/branchless idiom it
// uses for the equivalent guard `if(c) return NEW; return INIT;`: `<test c>; li result,INIT;
// b<!c>lr; li result,NEW; blr`. The false path returns the initializer already in the result; the
// true path falls through to the new value. emit_condition_test already yields branch-if-FALSE
// options, so no extra negation (the ^8 used by emit_guard_sequence's return-if-true case would be
// wrong here). Variable arms use a different move/staging form and still defer (not DIFF).
int ca_cmp(int a)            { int b = 0; if (a > 0)  b = 1; return b; }
int ca_eq(int a)            { int b = 2; if (a == 0) b = 8; return b; }
int ca_truth(int a)          { int b = 5; if (a)      b = 9; return b; }
unsigned ca_uns(unsigned a) { unsigned b = 1; if (a) b = 2; return b; }
