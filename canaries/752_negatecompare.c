// Negating a signed sign-bit comparison: `-(x < 0)` and `-(x > 0)` produce a 0/-1
// mask. mwcc reuses the comparison's sign-bit idiom but ends in an arithmetic shift
// (`srawi 31`, giving 0/-1) instead of the logical shift (`srwi 31`, giving 0/1) —
// the negation comes for free, with no separate `neg` and the operand left live for
// the `andc` of the `>` form.
int neg_lt(int x)        { return -(x < 0); }
int neg_gt(int x)        { return -(x > 0); }
int neg_gt_load(int *p)  { return -(*p > 0); }
