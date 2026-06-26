// A parameter live across a call, then STORED (not returned): `void f(int a){ foo(); gi =
// a; }`. mwcc saves the param to a callee-saved register up front and stores it from there
// after the call, with the epilogue reloading the saved LR BEFORE the GPR (the store-sink
// order, distinct from the return sink where the LR-reload hoist issues it after the call):
//
//     stwu r1,-16; mflr r0; stw r0,20(r1); stw r31,12(r1); mr r31,r3
//     bl foo ; stw r31,gi ; lwz r0,20(r1) ; lwz r31,12(r1) ; mtlr r0 ; addi r1,r1,16 ; blr
//
// The stored value may be computed from the saved register (`gi = a + 1`), and there may
// be several calls. Two saved values, the value also passed to the call, or a value both
// stored and returned defer to the general callee-saved allocator.
void foo(void);
int bar(int);
int gi;
void store_live(int a)         { foo(); gi = a; }        // mr r31,r3; bl; stw r31,gi
void store_computed(int a)     { foo(); gi = a + 1; }    // ...; addi r0,r31,1; stw r0,gi
void store_after_two(int a)    { foo(); foo(); gi = a; }
void store_after_arg_call(int a){ bar(0); gi = a; }      // the call's own arg is a constant
