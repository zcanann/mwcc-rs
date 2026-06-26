// Two constant stores to small-data (SDA21) globals — `gi = 5; gj = 7;`. mwcc
// materializes both constants up front (the first into a free register, the second into
// the scratch r0), then stores both: `li r3,5; li r0,7; stw r3,gi; stw r0,gj`. An SDA
// global store folds the relocation into the store (`stw r0, g@sda21`) — no base
// register, never writes the scratch — so a constant fill keeps its value live across it,
// exactly like the member/dereference fills the same path already handled. Widening
// is_scratch_safe_store_target to SDA integer globals was the whole fix; a repeated
// constant reuses one register, and a run of 3+ differing constants still defers (the
// scheduler interleaves those).
int gi, gj, gk;
void two_distinct(void)  { gi = 5; gj = 7; }   // li r3,5; li r0,7; stw r3; stw r0
void unused_param(int a) { gi = 5; gj = 7; }   // same — the dead parameter's r3 is free
void repeated(void)      { gi = 5; gj = 5; }   // li r0,5; stw r0; stw r0
void zeros(void)         { gi = 0; gj = 0; }   // li r0,0; stw r0; stw r0

// Three or more distinct constants to SDA globals extend the same pattern: mwcc loads
// them into r(N+1) down to r3, the last into r0, then stores all in source order —
// `li r4,1; li r3,2; li r0,3; stw r4; stw r3; stw r0`. A duplicate constant (which would
// share a register) and member/dereference targets still defer.
void three(void)        { gi = 1; gj = 2; gk = 3; }   // li r4,1; li r3,2; li r0,3; stw×3
