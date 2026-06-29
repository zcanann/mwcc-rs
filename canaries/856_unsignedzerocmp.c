// An unsigned value is always >= 0, so a comparison against literal 0 collapses: `u > 0` (and
// `0 < u`) is `u != 0`, and `u <= 0` (and `0 >= u`) is `u == 0`. mwcc emits the cheaper
// equality idiom for these; ours had used the unsigned relational idiom. (mwcc keeps `u >= 1`
// / `u < 1` as their OWN idioms, so those are not folded; and SIGNED `a > 0` is genuinely not
// `a != 0` — a can be negative — so signed comparisons are untouched.) Common in real code:
// `if (count > 0)`, `if (len <= 0)` on unsigned counts.
int u_gt_zero(unsigned a)        { return a > 0; }   // a != 0
int u_le_zero(unsigned a)        { return a <= 0; }  // a == 0
int zero_lt_u(unsigned a)        { return 0 < a; }   // a != 0
int zero_ge_u(unsigned a)        { return 0 >= a; }  // a == 0
int uchar_gt_zero(unsigned char a) { return a > 0; } // a != 0 (unsigned narrow)
