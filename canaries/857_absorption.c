// Boolean absorption: `(a & b) | a` is `a`, `(a | b) & a` is `a` (one operand subsumes the
// other). mwcc folds straight to the survivor — a bare `blr` (a already in r3). The bit-constant
// forms `(x | c) & c` and `(x & c) | c` are the same pattern with a constant survivor and fold
// to `li r3,c`. ours had emitted the full and/or pair. The inner op is dropped, so its operands
// and the surviving leaf must be side-effect-free leaves — `(h() & b) | h()` is two distinct
// calls and is left alone (it defers).
int absorb_or(int a, int b)    { return (a & b) | a; }  // -> a
int absorb_and(int a, int b)   { return (a | b) & a; }  // -> a
int absorb_or_comm(int a, int b){ return a | (a & b); } // -> a (survivor on the left)
int absorb_and_comm(int a, int b){ return a & (a | b); }// -> a
int absorb_match_q(int a, int b){ return (b & a) | a; } // -> a (matches the right inner operand)
int absorb_const_or(int a)     { return (a | 7) & 7; }  // -> 7 (li r3,7)
int absorb_const_and(int a)    { return (a & 7) | 7; }  // -> 7
