// In-place accumulator: `int t = p0 OP p1; t = t OP p2; …; return t;` where each
// operand after the first two is the NEXT parameter in register order and the
// accumulator is always the LEFT operand. mwcc keeps `t` in the result register and
// mutates it in place — `add r3,r3,r4; add r3,r3,r5` — because each source register
// dies at its single use, so r3 stays free for `t` and the left-operand anchor never
// moves. The value-tracking substitution model instead reassociates the folded chain
// (`(a+b)+c` -> `mr r0,r3; add r3,r4,r5; add r3,r0,r3`) and disagreed, so this exact
// shape deferred; try_inplace_accumulator now emits it directly. `+`/`-`/`*`, signed
// and unsigned, any chain length; a trailing unused parameter is fine.
//
// The divergent allocations still defer (the general allocator's job, roadmap): a
// first operand that is not p0 (`int t=c+d; t=t+a`) or the accumulator on the RIGHT
// (`t=c+t`) puts `t` in the scratch with a trailing `mr r3,r0`; a reused parameter
// (`t=t+a`) reserves its register so `t` cannot take r3.
int acc_add2(int a, int b, int c)            { int t = a + b; t = t + c; return t; }
int acc_add3(int a, int b, int c, int d)     { int t = a + b; t = t + c; t = t + d; return t; }
int acc_sub2(int a, int b, int c)            { int t = a - b; t = t - c; return t; }
int acc_mul2(int a, int b, int c)            { int t = a * b; t = t * c; return t; }
int acc_mixed(int a, int b, int c, int d)    { int t = a + b; t = t - c; t = t * d; return t; }
int acc_unused_tail(int a, int b, int c, int d) { int t = a + b; t = t + c; return t; }
unsigned acc_unsigned(unsigned a, unsigned b, unsigned c) { unsigned t = a + b; t = t + c; return t; }
