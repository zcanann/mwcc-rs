// Conditional-assign MOVE form (variable arm) — extends canary 889 (const-arm branch form). When
// either arm is a register-resident leaf variable, mwcc stages the initializer in a register and
// conditionally overwrites it: `<test c>; [li r0,INIT]; b<!c>fwd L; <mr/li> stage,NEW; L: mr
// result,stage; blr`. Const init stages in the scratch r0; a variable init stages in its OWN
// register (no li). The new value is `mr` (variable) or `li` (constant) into the stage register.
// The staged register must differ from the result — if the init variable already sits in the
// result, mwcc uses a different layout, so that case defers (verified, not DIFF). Non-leaf arms
// defer too. Registers are resolved before the compare is emitted so a deferral leaves no garbage.
int mv_cinit(int a)            { int b = 0; if (a) b = a; return b; }
int mv_vinit(int a, int c)     { int b = c; if (a) b = a; return b; }
int mv_vinit_cnew(int a, int c) { int b = c; if (a) b = 5; return b; }
