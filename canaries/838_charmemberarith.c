// A signed char/short struct member promoted to int is sign-extended (`extsb`). For a DIRECT
// return or a masking AND the member load is byte-exact (the return path adds the extsb; a
// mask makes it redundant). But a signed narrow member used as an ADDITIVE/MULTIPLICATIVE
// operand needs the extsb that the member load does not carry, and mwcc loads it into r0
// (an allocator register choice) — `p->x + 1` is `lbz r0; extsb r3,r0; addi`. That form is
// gated on the keystone register allocator, so `p->x + 1 / * 2 / - 3` DEFER (they were a
// miscompile: the raw zero-extended byte). An UNSIGNED member zero-extends on load (no
// extension needed); an int member is already 32-bit.
struct C { char x; };
struct U { unsigned char x; };
struct I { int x; };
int member_return(struct C* p) { return p->x; }       // lbz; extsb
int member_mask(struct C* p)   { return p->x & 0xf; } // lbz r0; clrlwi
int umember_add(struct U* p)   { return p->x + 1; }   // lbz (zero-extended); addi
int imember_add(struct I* p)   { return p->x + 1; }   // lwz; addi

// DEFERRED (signed narrow member in arithmetic, sign-extension + r0 register choice):
//   int f(struct C* p) { return p->x + 1; }
//   int f(struct C* p) { return p->x * 2; }
