// A guard with a COMPUTED fall-through — `if (cond) return CONST; return <expr>;` — compiles to
// a branch select, not a branchless one: mwcc stages the constant in r0, forward-branches past
// the computed arm when the condition selects the constant, evaluates the computed arm into r0,
// then `mr dest, r0`. So `if (a<0) return -1; return a+100;` is
// `cmpwi r3,0; li r0,-1; blt skip; addi r0,r3,100; skip: mr r3,r0; blr`. This is the common
// "error/bounds check, then the real computation" shape. The constant may be the early-return
// (true) arm or the fall-through (false) arm; the computed arm may read several variables.
int err_then_compute(int a)   { if (a < 0) return -1; return a + 100; }
int pos_guard(int a)          { if (a > 0) return 5;  return a + 1; }
int zero_guard(int a)         { if (a == 0) return 7; return a - 3; }
int const_false_arm(int a)    { if (a >= 0) return a + 1; return -1; }
int two_var_arm(int a, int b) { if (a < 0) return -1; return a + b; }
