// `return f(...) + x;` — a single general parameter `x` kept live across a call that sits inside the
// return expression, combined with the call's result. This is the callee-saved allocator's
// "value live across a call" case: mwcc saves x in r31 before the call (`mr r31,r3`), runs the call
// (whose argument, when it is x, is already in the incoming register — no move), reloads LR after the
// call, then combines from the saved register (`add r3,r31,r3`, the saved value first), and restores
// r31 in the epilogue. Frame is 16, saved_gpr_count 1.
//
// Covers: an argument-free call and a call forwarding the parameter, on either side of the `+`, for
// int and unsigned. DEFERS (no wrong bytes): a non-commutative op (`f()-x`, order-sensitive), a
// non-low-latency op (`f()*x`), and a multi-parameter shape — follow-up patterns.
int  g(void);
int  h(int);
int  add_call_result(int x)   { return g() + x; }   // add r3,r31,r3
int  add_forwarded(int x)     { return h(x) + x; }  // arg x already in r3, then add
int  add_param_first(int x)   { return x + g(); }   // same order as above (commutative)
unsigned gu(void);
unsigned add_unsigned(unsigned x) { return gu() + x; }
