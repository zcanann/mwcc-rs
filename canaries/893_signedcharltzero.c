// A SIGNED char load (deref/member/element) compared `< 0` is the 1-instruction sign-bit idiom and
// needs the char sign-extended first, like mwcc: `lbz r0; extsb r0,r0; srwi r3,r0,31`. The comparison
// idioms keep the value in the SCRATCH (extsb r0,r0 IN PLACE) — DIFFERENT from arithmetic (canary
// 892), which sign-extends into the destination via place_operand; using place_operand here would
// mismatch the register and double-extend. comparisons.rs `< 0` case now loads into the scratch and
// extends in place; the expressions.rs comparison pre-check is narrowed to allow `signed_char < 0`
// while the other relations (==0, >0, >=0, <=0, ==c, >c) and signed divide still DEFER (per-case work
// remains — they keep the value in the scratch too but with different idioms).
int deref_lt(char *p)         { return *p < 0; }
struct S { char x; int y; };
int member_lt(struct S *s)    { return s->x < 0; }
int elem_lt(char *a)          { return a[1] < 0; }
