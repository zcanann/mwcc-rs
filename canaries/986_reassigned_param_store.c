// A reassigned PARAMETER feeding memory stores folds exactly like a store-bearing
// local: the store value substitutes the tracked expression -- `x = x + 1; *p = x;`
// compiles as `*p = x + 1;` (`addi r0,r4,1; stw r0,0(r3)`). Reads before the
// reassignment keep the raw (pristine) register: `*p = x; x = x + 1; p[1] = x;` ->
// `stw r4,0(r3); addi r0,r4,1; stw r0,4(r3)`. (Previously "value tracking with stores
// or calls" deferred every Assign+Store mix without locals.)
int gi;

void reassign_store(int *p, int x)            { x = x + 1; *p = x; }
void reassign_store_two_reg(int *p, int x, int y) { x = x + y; *p = x; }
void reassign_global(int x)                   { x = x * 2; gi = x; }
void store_reassign_store(int *p, int x)      { *p = x; x = x + 1; p[1] = x; }
void local_param_mix(int *p, int x)           { int t = x + 1; x = t * 2; *p = x; }

// The fold composes with a LEADING early-return guard (passed through unchanged -- it
// executes before any reassignment, so it reads the pristine registers), the folded
// store landing in the guard-continuation schedule:
// `cmpwi; beq CONT; li r3,-1; blr; CONT: addi r0,r5,1; li r3,0; stw r0,0(r4); blr`.
int guarded_reassign_store(int a, int *p, int x)  { if (a) return -1; x = x + 1; *p = x; return 0; }
void void_guard_reassign(int a, int *p, int x)    { if (a) return; x = x + 1; *p = x; }
void void_guard_reassign_global(int a, int x)     { if (a) return; x = x * 2; gi = x; }
