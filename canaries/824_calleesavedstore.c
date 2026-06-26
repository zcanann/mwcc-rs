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

// The single saved value may also be RETURNED after the store — `int f(int a){ foo(); gi =
// a; return a; }` — `stw r31,gi; mr r3,r31; lwz r0,20; lwz r31,12; mtlr` (the LR-first
// epilogue still applies, the return move sits before it). Two saved values reschedule the
// epilogue (LR reload between the GPR reloads) and still defer.
int ret_and_store(int a)       { foo(); gi = a; return a; }       // store then return
int ret_store_computed(int a)  { foo(); gi = a; return a + 1; }   // return a value of the saved reg

// TWO saved values stored directly (leaves): `void f(int a,int b){ foo(); gi=a; gj=b; }`.
// mwcc reloads all-but-the-lowest saved GPR, then the saved LR, then the lowest GPR — for
// two values that is `lwz r31; lwz r0; lwz r30; mtlr` (the LR reload interleaved between
// the two GPR reloads). Three or more values reschedule that order (LR reload last) and
// defer; a computed store among two saved values defers (the single-value sink still
// allows a computed store).
int gk;
void store_two(int a, int b)       { foo(); gi = a; gj = b; }     // r31,r0,r30 epilogue
void store_two_swapped(int a, int b){ foo(); gi = b; gj = a; }    // saved regs by param order
