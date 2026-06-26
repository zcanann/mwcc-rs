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
