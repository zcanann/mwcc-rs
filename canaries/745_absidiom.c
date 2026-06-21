// The branchless abs idiom in its mirror forms: `x > 0 ? x : -x` and
// `x >= 0 ? x : -x` (the negate in the FALSE arm), plus the guard
// `if (x > 0) return x; return -x;` — all the same `srawi; xor; subf` mwcc emits
// for `x < 0 ? -x : x`. Only signed; an unsigned operand is not abs.
int absidiom_gt(int x)  { return x > 0 ? x : -x; }
int absidiom_ge(int x)  { return x >= 0 ? x : -x; }
int absidiom_guard(int x) { if (x > 0) return x; return -x; }
