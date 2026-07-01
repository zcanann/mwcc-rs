// `return f() - g();` — two argument-free calls whose results are subtracted in the return. The first
// call's result is live across the second, so mwcc saves it in r31 (`bl f; mr r31,r3`), runs the
// second call (`bl g`, result in r3), reloads LR, then `subf r3,r3,r31` (= r31 - r3 = f() - g()).
// Frame 16, saved_gpr_count 1.
//
// DEFERS (no wrong bytes): a COMMUTATIVE combine (`f()+g()`) — mwcc evaluates its operands right-first,
// reordering the symbol table, which the left-first symbol_order does not reproduce; a heavier op
// (`f()*g()`); and argument-bearing calls — follow-ups.
int f(void);
int g(void);
int diff(void)          { return f() - g(); }   // bl f; mr r31,r3; bl g; subf r3,r3,r31
unsigned uf(void);
unsigned ug(void);
unsigned udiff(void)    { return uf() - ug(); }
