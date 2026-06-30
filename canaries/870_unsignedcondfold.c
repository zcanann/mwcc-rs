// An unsigned-vs-zero comparison used as a CONDITION (in a select or guard) branches on the
// equality idiom, just like the value form (canary 856): `unsigned u > 0` is `u != 0` (`bne`),
// and `u <= 0` is `u == 0` (`beq`) — not the unsigned relational `bgt`/`ble`. emit_condition_test
// now applies that fold, so `(u>0) ? -1 : b+1` uses `bnelr`. Signed conditions and unsigned
// comparisons against non-zero bounds are unaffected. (It also benefits block-if conditions.)
int sel_ugt0(unsigned a, int b)  { return (a > 0) ? -1 : b + 1; }    // cmplwi; li -1; bnelr; addi
int sel_ule0(unsigned a, int b)  { return (a <= 0) ? -1 : b + 1; }   // beq idiom
int sel_0lt(unsigned a, int b)   { return (0 < a) ? -1 : b + 1; }    // commuted -> u != 0
int guard_ugt0(unsigned a)       { if (a > 0) return 1; return 0; }  // unsigned guard condition -> u != 0
