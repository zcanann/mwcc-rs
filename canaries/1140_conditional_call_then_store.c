// The VOID sibling of the conditional-call callee-saved shape (canary family around
// try_callee_saved_conditional_call): `void f(int cond, T* p) { if (cond) { calls } *p = <const>; }`.
// The trailing store's base parameter p is live across the CONDITIONAL call, so it parks in a
// callee-saved register (r31); the store runs from r31 at the join, before the epilogue. mwcc hoists
// the condition test into the mflr→save gap of the prologue:
//   stwu; mflr; cmpwi cond; stw r0,20; stw r31,12; mr r31,p; beq skip; bl g; skip: li r0,C;
//   stw r0,0(r31); lwz r0,20; lwz r31,12; mtlr; addi; blr
// This is the #21 (guard) × #20 (callee-saved) intersection for a void function. Scoped to ONE saved
// parameter and ONE constant store; a non-constant store value, a second trailing statement, the
// condition operand as the store base, or a call passing the saved parameter all defer. (fire 628)
void sink(void);
void ccs_truthy(int c, int* p) { if (c)      { sink(); } *p = 1; }  // cmpwi c,0; beq; ... ; stw r0,0(r31)
void ccs_eq(int c, int* p)     { if (c == 0) { sink(); } *p = 5; }  // bne skip (skip when c != 0)
void ccs_gt(int a, int* p)     { if (a > 0)  { sink(); } *p = 2; }  // ble skip (skip when a <= 0)
