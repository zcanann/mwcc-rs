// Two int locals each initialized by an argument-free call, with NO trailing call, combined in the
// return: `int x=g(); int y=h(); return x OP y;`. The FIRST result is live across the second call so
// it parks in one callee-saved register (r31); the SECOND stays in r3, and the return combines them
// straight from those registers: `bl g; mr r31,r3; bl h; <op> r3,r31,r3; <epilogue>`. Only ONE
// callee-saved register — distinct from try_callee_saved_call_result, whose model has a LATER call
// both locals cross (r30+r31). Low-latency combines only (+ - & | ^); a multiply reschedules the
// epilogue (scheduler-gated) and a reversed operand order (`y+x`) or a third local defer.
// Same or different callees both work — the calls run in statement order, so the relocation order is
// natural (no commutative right-first reorder like the direct `return f()+g();` form). (fire 626)
int g(void);
int h(void);
int tcr_add(void)  { int x = g(); int y = g(); return x + y; }  // add  r3,r31,r3
int tcr_addh(void) { int x = g(); int y = h(); return x + y; }  // two distinct callees, natural reloc order
int tcr_sub(void)  { int x = g(); int y = h(); return x - y; }  // subf r3,r3,r31 = x - y
int tcr_or(void)   { int x = g(); int y = g(); return x | y; }  // or   r3,r31,r3
int tcr_and(void)  { int x = g(); int y = g(); return x & y; }  // and  r3,r31,r3
int tcr_xor(void)  { int x = g(); int y = g(); return x ^ y; }  // xor  r3,r31,r3
