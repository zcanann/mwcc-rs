// A SIGNED CHAR load (deref/element/member) with a fitting constant for `|`, `^`, or `<<`: mwcc keeps
// the sign-extended byte in the SCRATCH, then the immediate op reads r0 into the destination
// (`lbz r0; extsb r0,r0; ori|xori|slwi r3,r0,c`). Unlike `addi`, ori/xori/slwi can source r0, so the
// scratch convention (the new inline branch in emit_constant_form, using signed_byte_scratch_source)
// applies — completing the char-deref arithmetic seam alongside +/-/~/-/!/>>. Multiply (`*p * c`)
// still defers (its power-of-two-to-slwi split needs the generic path). Unchanged: `*p + 1` keeps the
// destination convention (addi); `*p & 0xf` is the raw-byte mask; `unsigned char` needs no extsb.
int set_high(char *p)        { return *p | 0x80; }
int toggle(char *p)          { return *p ^ 7; }
int pack(char *p)            { return *p << 2; }
int elem_or(char *a)         { return a[2] | 0x40; }
struct S { char x; int y; };
int member_xor(struct S *s)  { return s->x ^ 3; }
