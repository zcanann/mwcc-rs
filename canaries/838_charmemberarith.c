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

// DEFERRED — a signed narrow member promoted to int needs the extsb its load does not carry,
// with mwcc's r0-load register choice gated on the keystone allocator. Every constant-form
// operator routed through emit_constant_form defers, plus the full/wide mask:
//   p->x + 1   p->x - 3   p->x * 2   p->x << 2   p->x | 5   p->x ^ 5
//   p->x & 0xff (full byte: mwcc drops the redundant mask)   p->x & 0x100 (reaches sign bit)
// STILL OPEN (separate codegen paths, not yet deferred): p->x >> 1, p->x / 2, -p->x, ~p->x,
// p->x > 0 — the place_operand chokepoint catches these but needs the mask exemption wired.
