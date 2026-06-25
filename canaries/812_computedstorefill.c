// Two computed-value stores to distinct integer SDA globals. mwcc overlaps the two
// value computations — it evaluates both first (the earlier into a real GPR, the later
// into the scratch r0), then stores both — rather than the sequential `compute; store;
// compute; store`. This is the first cross-statement use of the vreg allocator: the
// first value is emitted into a fresh virtual, and the allocator gives it the in-place
// GPR while keeping it off r0 (it is live across the second computation); the second
// value goes into r0 directly. So `gi = a+1; gj = b*2;` is:
//
//     addi r3,r3,1   ; a+1 -> r3 (a dies, in place; off r0 since live across the next op)
//     slwi r0,r4,1   ; b*2 -> r0 (the transient)
//     stw  r3,gi
//     stw  r0,gj
//
// Restricted to the shape that stays byte-exact without the full Phase-E scheduler: each
// value is a single LOW-latency op (add/sub/and/or/xor/shift/neg/not, or a power-of-two
// multiply = shift) over register-resident operands (parameters and constants). A
// multi-cycle op (register or non-power-of-two multiply, divide, modulo) reorders mwcc's
// compute/store schedule by readiness; a memory read needs load hoisting; a float global
// uses the float path; a member/array target, a repeated target (dead-store), and 3+
// stores each stay on their own path. All of those defer to the normal path unchanged.
int gi, gj, gk;
void two_adds(int a, int b)            { gi = a + 1; gj = b + 2; }   // addi r3; addi r0; stw; stw
void add_then_shift(int a, int b)      { gi = a + 1; gj = b * 2; }   // addi r3; slwi r0; stw; stw
void logical(int a, int b)             { gi = a & 7; gj = b | 3; }   // andi.; ori; stw; stw
void two_operand(int a, int b, int c)  { gi = a + b; gj = b - c; }   // add; subf; stw; stw
void with_negate(int a, int b)         { gi = -a;    gj = b + 1; }   // neg; addi; stw; stw
