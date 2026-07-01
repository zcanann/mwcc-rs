// `return f(...) OP x;` — a single general parameter `x` kept live across a call that sits inside the
// return expression, combined with the call's result. This is the callee-saved allocator's
// "value live across a call" case: mwcc saves x in r31 before the call (`mr r31,r3`), runs the call
// (whose argument, when it is x, is already in the incoming register — no move), reloads LR after the
// call, then combines from the saved register (`OP r3,r31,r3`, the saved value first), and restores
// r31 in the epilogue. Frame is 16, saved_gpr_count 1.
//
// Covers: the low-latency ops `+ | & ^` (commutative — `OP r3,r31,r3` on either source side), `-`
// (non-commutative — the `subf` operands follow the call's side), and `*` (a single `mullw r3,r31,r3`
// that mwcc issues BEFORE the LR reload to overlap its latency — the LR-reload hoist models this),
// an argument-free call and a call forwarding the parameter, on either side, for int and unsigned.
// DEFERS (no wrong bytes): a multi-parameter shape — follow-up.
int  g(void);
int  h(int);
int  add_call_result(int x)   { return g() + x; }   // add   r3,r31,r3
int  add_forwarded(int x)     { return h(x) + x; }  // arg x already in r3, then add
int  add_param_first(int x)   { return x + g(); }   // same order as above (commutative)
int  or_call_result(int x)    { return g() | x; }   // or    r3,r31,r3
int  and_call_result(int x)   { return g() & x; }   // and   r3,r31,r3
int  xor_forwarded(int x)     { return x ^ h(x); }  // xor   r3,r31,r3
int  sub_call_left(int x)     { return g() - x; }   // subf  r3,r31,r3  (result - param)
int  sub_call_right(int x)    { return x - h(x); }  // subf  r3,r3,r31  (param - result)
int  mul_call_result(int x)   { return g() * x; }   // bl; mullw r3,r31,r3; lwz r0 (reload after mullw)
int  mul_forwarded(int x)     { return x * h(x); }  // mullw after the call, before the LR reload
unsigned gu(void);
unsigned add_unsigned(unsigned x) { return gu() + x; }
unsigned sub_unsigned(unsigned x) { return x - gu(); }
