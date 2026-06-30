// Signed char load `== c` (nonzero constant) — e.g. `*p == '\n'`, the character-match check.
// mwcc: `lbz r0; extsb r0,r0; subfic r0,r0,c; cntlzw r0,r0; srwi r3,r0,5` (value in scratch). The
// a==c leading-zeros case (comparisons.rs) errored on a deref (general_register_of_leaf); now a
// signed byte loads into the scratch with an in-place `extsb r0,r0` before the subfic. The
// comparison pre-check (expressions.rs) allows `signed_char == <small const>`. Works for positive,
// negative, and character-literal constants. `> c`/`< c` (complex idiom), `!= c` (deref not a leaf),
// signed divide, and the unsigned-char `== c` deref still DEFER (not DIFF).
int eq_const(char *p)         { return *p == 5; }
int eq_neg(char *p)           { return *p == -1; }
int eq_char(char *p)          { return *p == 'A'; }
struct S { char x; int y; };
int member_eq_const(struct S *s) { return s->x == 10; }
