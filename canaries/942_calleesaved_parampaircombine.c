// `g(x); return x OP y;` — TWO parameters both live across a single call: the first is passed to the
// call, the second is only used in the return, and both are combined by a commutative low-latency op.
// mwcc preserves both in callee-saved registers — the LAST parameter in r31, the first in r30 — saving
// them interleaved up front (`stw r31; mr r31,y; stw r30; mr r30,x`). The call finds the first
// parameter still in its incoming register (no move); the return combines from the saved registers
// (`add r3,r30,r31`, operand order following the source side). Frame 16, saved_gpr_count 2.
//
// DEFERS (no wrong bytes): passing the SECOND parameter (`g(y)` needs `mr r3,r31`), a non-commutative
// op (`x-y`), and a return that does not read both distinct parameters — follow-ups.
void g(int);
int add_xy(int x, int y) { g(x); return x + y; }   // add r3,r30,r31
int add_yx(int x, int y) { g(x); return y + x; }   // add r3,r31,r30 (source order)
int or_xy(int x, int y)  { g(x); return x | y; }   // or  r3,r30,r31
int and_xy(int x, int y) { g(x); return x & y; }   // and r3,r30,r31
