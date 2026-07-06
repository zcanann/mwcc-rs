// `(X ± c) + Y` — a `variable ± constant` plus a register-leaf variable — reassociates to
// `(X + Y) ± c`: mwcc groups the two register terms in SOURCE order and applies the constant last
// (`(a-1)+b` -> `add r3,r3,r4; addi r3,r3,-1`; `(b-1)+a` -> `add r3,r4,r3; addi r3,r3,-1`).
// evaluate_general reproduces this directly, so these compile byte-exact instead of deferring.
// A both-`±c` right operand (`(a-1)+(b-1)`, which mwcc orders `add r3,r4,r3`) and a global/memory
// leaf still defer (different var order / no ready register).
int hoist_sub(int a, int b)      { return (a - 1) + b; }
int hoist_sub_rev(int a, int b)  { return (b - 1) + a; }
int hoist_add(int a, int b)      { return (a + 1) + b; }
int hoist_self(int a)            { return (a - 1) + a; }
int hoist_big(int a, int b)      { return (a - 100) + b; }
int hoist_three(int a, int b, int c) { return (a - 1) + c; }
// The `± c` term on the RIGHT is the same emit — its variable is still the first `add` operand.
int hoist_right(int a, int b)    { return b + (a - 1); }   // add r3,r3,r4; addi -1
int hoist_right2(int a, int b)   { return a + (b - 1); }   // add r3,r4,r3; addi -1
// BOTH operands `±c`: the SECOND term's variable is first, the (signed) constants sum.
int hoist_both(int a, int b)     { return (a - 1) + (b - 1); } // add r3,r4,r3; addi -2
int hoist_both_mix(int a, int b) { return (a + 1) + (b - 3); } // add r3,r4,r3; addi -2
int hoist_both_rev(int a, int b) { return (b - 1) + (a - 1); } // add r3,r3,r4; addi -2
// `(X + Y) - c` (sum minus const): mwcc pushes -c into the SECOND operand and adds the first,
// saving the first to r0 only when it occupies the destination.
int sub_sum_saved(int a, int b)  { return (a + b) - 1; }   // mr r0,r3; addi r3,r4,-1; add r3,r0,r3
int sub_sum_inplace(int a, int b){ return (b + a) - 1; }   // addi r3,r3,-1; add r3,r4,r3
int sub_sum_three(int a, int b, int c) { return (a + c) - 1; }
