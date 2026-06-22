// A signed `char` used as a condition is sign-extended with the record-form `extsb.`
// (which sets cr0) — mwcc loads it with `lbz` (zero-extend) then `extsb.`, doing the
// sign-extend and the zero-test in one. ours emitted `lbz; cmpwi` (no sign-extend),
// differing by an instruction. unsigned char (cmplwi), short (lha sign-extends on
// load), int, and pointer conditions are unaffected.
struct S { char sb; unsigned char ub; short sh; int i; };
void sink(void);
void on_schar(struct S *p) { if (p->sb)  sink(); }
void on_uchar(struct S *p) { if (p->ub)  sink(); }
void on_not_schar(struct S *p) { if (!p->sb) sink(); }
