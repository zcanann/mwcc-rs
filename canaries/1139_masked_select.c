// The branchless MASKED SELECT `(a REL 0) ? b : 0` / `(a REL 0) ? 0 : b`, where b is a leaf DIFFERENT
// from the condition operand a. mwcc builds the sign mask of a (all-ones exactly when the relation
// holds) and combines it with b via `and` (`? b : 0`) or `andc` (`? 0 : b`) — no branch:
//   < 0 : srawi r0,a,31
//   > 0 : neg r0,a; andc r0,r0,a; srawi r0,r0,31
//   <= 0: neg r0,a; orc r0,a,r0; srawi r0,r0,31
//   >= 0: srwi a,a,31; addi r0,a,-1   (a is dead after the compare, so its register holds the flag)
// then `and`/`andc r3,b,r0`. This is try_emit_sign_clamp (`(a REL 0)?a:0`) generalized to a distinct
// value b (try_emit_masked_select). Restricted to a signed operand and an in-register destination:
// an unsigned operand, a store (scratch destination), a non-zero compare constant, and the clamp form
// (b == a, still owned by try_emit_sign_clamp) all defer. (fire 627 — general #21)
int msel_gt(int a, int b)   { return (a > 0)  ? b : 0; }  // neg; andc; srawi; and r3,r4,r0
int msel_lt(int a, int b)   { return (a < 0)  ? b : 0; }  // srawi r0,r3,31; and r3,r4,r0
int msel_ge(int a, int b)   { return (a >= 0) ? b : 0; }  // srwi r3,r3,31; addi r0,r3,-1; and r3,r4,r0
int msel_le(int a, int b)   { return (a <= 0) ? b : 0; }  // neg; orc; srawi; and r3,r4,r0
int msel_gt_c(int a, int b) { return (a > 0)  ? 0 : b; }  // neg; andc; srawi; andc r3,r4,r0
int msel_lt_c(int a, int b) { return (a < 0)  ? 0 : b; }  // srawi r0,r3,31; andc r3,r4,r0
