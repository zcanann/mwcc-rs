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
int member_return(struct C* p) { return p->x; }        // lbz; extsb
int member_mask(struct C* p)   { return p->x & 0xf; }  // lbz r0; clrlwi (partial mask)
int member_mask2(struct C* p)  { return p->x & 0x7f; } // partial mask within the byte
int umember_add(struct U* p)   { return p->x + 1; }    // lbz (zero-extended); addi
int imember_add(struct I* p)   { return p->x + 1; }    // lwz; addi
int g;
void member_if(struct C* p)    { if (p->x > 0) g = 1; }  // CONDITION form: byte-exact (extsb; ble)
void member_truthy(struct C* p){ if (p->x) g = 1; }      // truthiness: byte-exact

// CLUSTER CLOSED — a signed narrow member promoted to int needs the extsb its load does not
// carry; mwcc loads it into r0 and sign-extends into the destination, a register choice gated
// on the keystone allocator. Every operator that takes the member as a DIRECT integer operand
// now defers rather than miscompile on the raw zero-extended byte (`p->x = 0xFF` reads 255):
//   p->x + 1  - 3  * 2  << 2  >> 1  | 5  ^ 5  / 2  % 4    (arith / shift / divide)
//   -p->x  ~p->x                                          (unary, via place_operand)
//   p->x > 0  < 5  == 1  != 0   p->x + p->y  p->x ? 1 : 2 (compare / reg-form / ternary)
//   p->x & 0xff (full byte: mwcc drops it)  & 0x100 (reaches the sign bit)
// EXEMPT (byte-exact): a STRICT partial mask (`& 0xf`, `& 0x7f`), the direct return, an
// unsigned member (zero-extends), an int member, and the CONDITION form `if (p->x > 0)`.
