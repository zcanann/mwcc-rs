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
