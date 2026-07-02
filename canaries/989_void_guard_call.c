// A bare VOID early return over a call continuation in a NON-LEAF function: the guard
// folds to a single INVERTED conditional branch straight to the shared epilogue --
// `stwu; mflr; cmpwi; stw r0; bne EPILOGUE; bl g; EPILOGUE: lwz; mtlr; addi; blr` --
// not a skip over an unconditional branch (which previously shipped one extra `b`, a
// miscompile). Covers plain, argument, multi-call, and function-pointer continuations.
extern void g(void);
extern void h(void);
extern void ex(int);
typedef void (*handler)(int);

void guard_call(int a)              { if (a) return; g(); }
void guard_arg_call(int a)          { if (a) return; ex(0); }
void guard_two_calls(int a)         { if (a) return; g(); h(); }
void guard_cmp_call(int a, int b)   { if (a > b) return; ex(b); }
void guard_fn_pointer(handler p, int s) { if (!p) return; p(s); }
