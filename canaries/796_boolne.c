// `x != 0` (and the ternary `x ? 1 : 0` / `!!x` that lower to it) booleanizes via the
// sign bit of `-x | x` — the top bit is set iff x has any bit set. The negate must
// target the scratch while x stays in its own register; computing x into the
// destination when that is the scratch (a store, d=r0) let the neg clobber x, so the
// `or` became `or r0,r0,r0` and the result collapsed to `(-x) >> 31` — a miscompile
// (wrong for any x whose negation flips the top bit, e.g. a negative or high value).
int b1, b2, b3;
void ne0(int a)        { b1 = a != 0; }    // neg r0,r3; or r0,r0,r3; srwi r0,r0,31
void tern(unsigned a)  { b2 = a ? 1 : 0; }
void bnot(int a)       { b3 = !!a; }
int  ret_ne0(int a)    { return a != 0; }
