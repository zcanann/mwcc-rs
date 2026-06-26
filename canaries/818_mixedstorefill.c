// Two stores to distinct integer SDA globals where one value is a single-op register
// computation and the other a register-resident leaf parameter — `gi = a+1; gj = b;`.
// mwcc computes the computed value into the scratch, then stores the LEAF first (it is
// ready immediately while the computed result settles), then the computed value:
//
//     addi r0,r3,1   ; a+1 -> scratch
//     stw  r4,gj     ; the leaf b, already in its register, stored first
//     stw  r0,gi     ; the computed value second
//
// (Was a diff: ours emitted source order. The both-computed case is the latency-ordered
// fill; both-leaf is the normal path; both-constant is the constant fill.) The leaf is
// stored first regardless of source position, and regardless of the computed value's
// latency. A global/memory leaf needs a load (defers).
int gi, gj;
void computed_then_leaf(int a, int b)        { gi = a + 1; gj = b; }   // addi r0; stw r4; stw r0
void leaf_then_computed(int a, int b)        { gi = a;     gj = b + 1; } // addi r0; stw r3; stw r0
void high_latency(int a, int b)              { gi = a * b; gj = b; }   // mullw r0; stw; stw r0
void two_operand(int a, int b, int c)        { gi = a + b; gj = c; }   // add r0; stw; stw r0

// The filler may also be a constant — `gi = a; gj = 5;` — same shape: `li r0,5; stw
// r3,gi; stw r0,gj`, the leaf stored first, the constant second, regardless of source
// order. (Both-constant is the constant fill; computed+constant is the computed fill.)
void leaf_then_const(int a)                  { gi = a; gj = 5; }    // li r0,5; stw r3; stw r0
void const_then_leaf(int a)                  { gi = 5; gj = a; }    // li r0,5; stw r3,gj; stw r0,gi
