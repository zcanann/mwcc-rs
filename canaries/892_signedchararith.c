// A SIGNED char load (deref `*p`, member `s->x`, element `a[i]`) used in integer Add/Subtract is
// now sign-extended like mwcc: `lbz r0; extsb d,r0; addi/subf`. place_operand (expressions.rs)
// materializes a signed byte sign-extended into the destination for a real-register destination
// (it previously deferred, citing the r0-load as keystone — but it is just the scratch); the
// emit_constant_form gate (arithmetic.rs) now allows Add/Subtract through that path. The other
// operators (multiply/shift/or/xor) take a different operand path that does not sign-extend yet,
// and the scratch (value/store) destination uses a different layout — both still DEFER (not DIFF).
// Unsigned char, short, the fitting-mask `& 0xf`, and the plain `return *p` were already byte-exact.
int deref_add(char *p)            { return *p + 1; }
int deref_sub(char *p)            { return *p - 3; }
struct S { char x; int y; };
int member_add(struct S *s)       { return s->x + 1; }
int elem_add(char *a, int i)      { return a[i] + 1; }
