// A cast to a narrow integer type (`(unsigned char)`, `(char)`, `(unsigned short)`) emits its
// own narrowing — clrlwi for the unsigned mask, extsb/extsh for the signed extend. So when its
// operand is a narrow LEAF (a char/short param or local), that operand is read RAW: the leaf's
// promotion extsb would be immediately overridden by the cast's widen, so mwcc omits it.
// `(unsigned char)a` is a bare `clrlwi r3,r3,24`, and `(char)char_a` is one `extsb`, not the
// `extsb r0,r3; clrlwi r3,r0,24` / double-extsb ours emitted before.
int uchar_of_char(char a)    { return (unsigned char)a + 1; }   // clrlwi r3,r3,24; addi
int uchar_ret(char a)        { return (unsigned char)a; }       // clrlwi r3,r3,24
int char_noop(char a)        { return (char)a + 1; }            // extsb r3,r3; addi  (one extsb)
int ushort_of_short(short a) { return (unsigned short)a + 1; }  // clrlwi r3,r3,16; addi
int uchar_of_short(short a)  { return (unsigned char)a + 1; }   // clrlwi r3,r3,24; addi

// Unaffected: an int operand needs no raw read (already wide), and a cast of a pointer LOAD
// (`(unsigned char)*p`) or a char GLOBAL keeps its existing path (the load's register choice /
// the lbz-already-zero-extends fold are separate, not this redundant-extsb case).
