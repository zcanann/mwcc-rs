// The r31+r30 PAIR: a memory-loaded local AND a parameter both live across a call --
// `int t = gi; g(); return t + s;` saves both callee-saved registers, parks s in r30
// and the load in r31, and computes the return in the epilogue slot after the LR
// reload: `stw r31,12; stw r30,8; mr r30,r3; lwz r31,gi; bl; lwz r0,20;
// add r3,r31,r30; lwz r31; lwz r30; mtlr; addi; blr`.
// And a guarded fn-pointer call whose argument ALREADY sits in its argument register
// (`t(s)` with s the first parameter) is byte-identical to the zero-argument form.
typedef void (*handler)(int);
handler gh;
int gi;
extern void g(void);

int pair_add(int s)      { int t = gi; g(); return t + s; }
int pair_add_swapped(int s) { int t = gi; g(); return s + t; }
int pair_sub(int s)      { int t = gi; g(); return t - s; }
void guarded_arg_call(int s) { handler t = gh; if (!t) return; t(s); }
