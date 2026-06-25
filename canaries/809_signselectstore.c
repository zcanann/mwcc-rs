// `(x REL 0) ? c1 : c2` with consecutive constants, stored: the shifted sign bit lands in
// the value's own (now-dead) register, then `addi` carries the offset to the destination
// — `srawi r3,r3,31; addi r0,r3,2; stw r0`. The `> 0` case prepends `neg r0,r3; andc
// r0,r0,r3` (the scratch), which is why the shift must avoid r0. Previously the `> 0`
// store was gated off entirely (deferred), and `< 0`/`>= 0` stores wrote the shift into
// the destination scratch (a diff); both now match mwcc. Return form (a's reg == dest)
// is unchanged.
int gi;
void gt(int a)  { if (a > 0)  gi = 1; else gi = 2; }  // neg; andc; srawi r3; addi r0,r3,2
void lt(int a)  { if (a < 0)  gi = 1; else gi = 2; }  // srawi r3,r3; addi r0,r3,2
void ge(int a)  { if (a >= 0) gi = 1; else gi = 2; }  // srwi/xori path + addi
int  ret(int a) { if (a > 0) return 1; return 2; }    // return form — unchanged
