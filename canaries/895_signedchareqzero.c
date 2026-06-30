// Signed char load (deref/member/element) `== 0` (the null-terminator check), extending canaries
// 893/894. mwcc keeps the sign-extended byte in the scratch: `lbz r0; extsb r0,r0; cntlzw r0,r0;
// srwi r3,r0,5`. The emit_comparison ==0 leading-zeros case (comparisons.rs) previously routed the
// operand through place_operand_or_scratch (which, after the arithmetic fix 43440c9, sign-extends
// into the DESTINATION) and then ran its own extsb -> a DOUBLE extsb + wrong register. Now a signed
// byte loads into the scratch with a single in-place `extsb r0,r0`, and the leading-zero test reads
// the scratch. The comparison pre-check (expressions.rs) allows Equal-against-zero for a signed char.
// `<= 0` (cntlzw+rlwnm), `== c` (subfic), `> c`, and signed divide still DEFER (not DIFF).
int eq0(char *p)              { return *p == 0; }
struct S { char x; int y; };
int member_eq0(struct S *s)   { return s->x == 0; }
int elem_eq0(char *a)         { return a[1] == 0; }
