// `g(x); return x OP y;` — TWO parameters both live across a single call: one is passed to the call,
// the other only used in the return, and both are combined by a commutative low-latency op. mwcc
// preserves both in callee-saved registers — the LAST parameter in r31, the first in r30 — saving them
// interleaved up front (`stw r31; mr r31,y; stw r30; mr r30,x`). The call may pass EITHER parameter:
// the first stays in its incoming register (no move); the second is materialized from its saved r31
// (`mr r3,r31`). The return combines from the saved registers (`add r3,r30,r31`, operand order
// following the source side). Frame 16, saved_gpr_count 2.
//
// The commutative ops (`+ | & ^`) and `-` (subf order following the source side) are handled. DEFERS
// (no wrong bytes): multiply — with two saved GPRs mwcc interleaves the LR reload between the register
// restores (`mullw; lwz r31; lwz r0; lwz r30`), a register-death epilogue this path does not model —
// and a return not reading both distinct parameters — follow-ups.
void g(int);
int add_xy(int x, int y)  { g(x); return x + y; }   // pass 1st: no arg move; add r3,r30,r31
int add_yx(int x, int y)  { g(x); return y + x; }   // add r3,r31,r30 (source order)
int or_xy(int x, int y)   { g(x); return x | y; }   // or  r3,r30,r31
int and_xy(int x, int y)  { g(x); return x & y; }   // and r3,r30,r31
int add_pass2(int x, int y) { g(y); return x + y; } // pass 2nd: mr r3,r31; add r3,r30,r31
int xor_pass2(int x, int y) { g(y); return y ^ x; } // pass 2nd; xor r3,r31,r30
int sub_xy(int x, int y)  { g(x); return x - y; }   // subf r3,r31,r30 (x - y, source order)
int sub_yx(int x, int y)  { g(x); return y - x; }   // subf r3,r30,r31 (y - x)
int sub_pass2(int x, int y) { g(y); return x - y; } // pass 2nd + subtract
