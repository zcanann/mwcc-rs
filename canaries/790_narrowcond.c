// A narrow leaf operand (a `short`/`char` parameter in a register) is sign/zero-
// extended before a comparison or truth test — mwcc emits extsh/extsb/clrlwi, and
// against zero the record form (extsh./extsb./clrlwi.) folds the extension and the
// compare into one. (A two-operand narrow compare, which extends both, is deferred.)
void sink(void);
void sl(short s)          { if (s < 0)     sink(); }   // extsh. (record, vs zero)
void sg(short s)          { if (s > 5)     sink(); }   // extsh + cmpwi
void st(short s)          { if (s)         sink(); }   // extsh. (truthiness)
void ug(unsigned char c)  { if (c > 0x80)  sink(); }   // clrlwi + cmplwi
void uz(unsigned char c)  { if (c == 0)    sink(); }   // clrlwi. (record)
void ut(unsigned char c)  { if (c)         sink(); }   // clrlwi. (truthiness)
void cl(signed char c)    { if (c < 0)     sink(); }   // extsb. (record)
void uh(unsigned short u) { if (u > 100)   sink(); }   // clrlwi(16) + cmplwi
// A *signed* char member loads with lbz (zero-extends), so mwcc re-extends in place
// with extsb before the compare; other narrow members (lha/lhz/lbz) come back correct.
struct Pak { int flags; signed char level; };
void ml(struct Pak* p)    { if (p->level < 0) sink(); }   // lbz; extsb. (record)
void mg(struct Pak* p)    { if (p->level > 5) sink(); }   // lbz; extsb; cmpwi
