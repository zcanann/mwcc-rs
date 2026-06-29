// `(unsigned char)` of a char LOAD folds to a bare byte load. The `lbz`/`lbzx` already
// zero-extends the byte to 0..255 — exactly the unsigned-char value — so mwcc drops BOTH the
// signed-char promotion extsb and the cast's own `& 0xff` clrlwi, leaving just the load. ours
// had emitted the redundant extsb and/or clrlwi (the global differed; the dereference and
// member deferred). The fold applies to a signed-char global, dereference, member, or array
// element; a short load still needs the `& 0xff` (its load is wider), and a char-LEAF cast is
// byte-exact via the raw-read path (canary 843).
char gc;
struct S { char x; };
int uchar_global(void)         { return (unsigned char)gc; }       // lbz r3,gc  (bare)
int uchar_global_arith(void)   { return (unsigned char)gc + 1; }   // lbz; addi
int uchar_deref(char* p)       { return (unsigned char)*p; }       // lbz r3,0(r3)
int uchar_deref_arith(char* p) { return (unsigned char)*p + 1; }   // lbz; addi
int uchar_member(struct S* p)  { return (unsigned char)p->x; }     // lbz
int uchar_member_arith(struct S* p) { return (unsigned char)p->x + 1; }
