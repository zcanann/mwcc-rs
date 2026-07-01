// `p(x); q(y);` — two single-argument calls passing two distinct parameters in order, in a void body.
// The SECOND parameter is live across the FIRST call, so mwcc preserves it in a callee-saved register:
// `mr r31,r4` up front, run the first call (the first parameter is still in its incoming r3, no move),
// then `mr r3,r31; bl` for the second call. The epilogue reloads LR (hoisted right after the last call)
// then restores r31. Frame 16, saved_gpr_count 1.
//
// DEFERS (no wrong bytes): a reversed order (`p(y); p(x)`), a repeated argument (`p(x); p(x)`), and
// multi-argument calls — each a different live/register shape, follow-ups.
void p(int);
void q(int);
void two_calls_same(int x, int y) { p(x); p(y); }   // mr r31,r4; bl p; mr r3,r31; bl p
void two_calls_diff(int x, int y) { p(x); q(y); }   // same shape, different callees
