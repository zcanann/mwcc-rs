// REGRESSION FIX: a signed char load (deref/member/element) in a unary (`~`, `-`, `!`) or
// shift-right (`>>`) idiom must keep the sign-extended byte in the SCRATCH, where those idioms read
// it: `lbz r0; extsb r0,r0; <neg|not|cntlzw|srawi> r3,r0`. The 43440c9 place_operand fix made
// place_operand sign-extend into the DESTINATION (correct for `addi`, which cannot take r0 as a
// source) and un-deferred these idioms, which then read the wrong register — a byte diff shipped
// since 43440c9. New signed_byte_scratch_source helper (expressions.rs) does the scratch+in-place-
// extsb; the negate/bitnot/logical-not (expressions.rs) and shift-right (arithmetic.rs) idioms use
// it. Add/Subtract keep the destination convention via place_operand. Multiply/shift-left/or/xor/
// modulo still defer (NOT DIFF). (`unsigned char >>` is a separate pre-existing DIFF, untouched.)
int complement(char *p)      { return ~*p; }
int negate(char *p)          { return -*p; }
int logical_not(char *p)     { return !*p; }
int shift_right(char *p)     { return *p >> 1; }
struct S { char x; int y; };
int member_neg(struct S *s)  { return -s->x; }
