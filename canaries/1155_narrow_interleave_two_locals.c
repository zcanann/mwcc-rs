// The 2-LOCAL slice of the __va_arg init-interleave (extends canary 1154's 1-local form): two
// const-init locals both reassigned to constants in one narrow-guarded block, returned as their sum.
// The homes mirror the direct `a+b` lowering (a in r3 — the in-place add — and b in r0); the width op
// leads, a's init fills the latency gap, the LOGICAL compare consumes the scratch, b's init lands in
// the freed r0, the arm rewrites both, the join adds:
//   clrlwi r0,t,24; li r3,8; cmplwi r0,2; li r0,4; bne L; li r3,7; li r0,8; L: add r3,r3,r0; blr
// Three-plus locals REASSOCIATE the sum (a+(b+c): c->r3, b->r0, a->r4) — deferred, the next slice.
// (fire 646)
int nil2_eq(unsigned char t) { int a = 8; int b = 4; if (t == 2) { a = 7; b = 8; } return a + b; }
int nil2_gt(unsigned char t) { int a = 8; int b = 4; if (t > 3)  { a = 7; b = 8; } return a + b; }
// THREE locals (fire 647): the sum REASSOCIATES a+(b+c), shifting the consumer-tree homes — c takes
// the in-place r3, b the scratch r0, and a the first volatile past the (single) parameter, r4:
//   clrlwi r0,t,24; li r4,8; cmplwi r0,2; li r0,4; li r3,1; bne L; li r4,7; li r0,9; li r3,2;
//   L: add r3,r0,r3; add r3,r4,r3; blr
// A SECOND parameter defers (mwcc reclaims a DEAD extra param's register — allocator liveness).
int nil3(unsigned char t) { int a = 8; int b = 4; int c = 1; if (t == 2) { a = 7; b = 9; c = 2; } return a + b + c; }
