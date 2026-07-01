// `gi * gi` / `gi + gi` for an integer GLOBAL reads the same memory on both sides. A global read is a
// LOAD (unlike a register-resident parameter, a free re-read), so mwcc loads it ONCE and applies the op
// to that register twice (`lwz r0,gi; mullw r3,r0,r0`) — not two loads. The codegen reproduces this
// load-once reuse for `+` and `*`.
//
// DEFERS (no wrong bytes): a signed plain `char` global (its `lbz` needs a trailing `extsb` this path
// omits) and the shift operators (`g << g`) — narrow / rare.
int            gi;
unsigned       gu;
short          gs;
unsigned char  gb;
int      isquare(void)          { return gi * gi; }   // lwz r0,gi; mullw r3,r0,r0
int      idouble(void)          { return gi + gi; }   // lwz r0,gi; add   r3,r0,r0
unsigned usquare(void)          { return gu * gu; }
int      ssquare(void)          { return gs * gs; }   // lha r0,gs; mullw r3,r0,r0 (self-extending)
int      bsquare(void)          { return gb * gb; }   // lbz r0,gb (unsigned, no extsb)
int      param_square(int x)    { return x * x; }     // a register value: mullw r3,r3,r3 (no load)
