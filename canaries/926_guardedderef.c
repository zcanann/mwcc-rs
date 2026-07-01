// A null-guarded dereference — `if (!p) return CONST; return <access of p>;` or the mirror
// `if (p) return <access of p>; return CONST;` — is byte-exact for an int-width return. mwcc CANNOT
// fold it to a branchless `p ? access : CONST` (dereferencing a null pointer is unsafe), so it emits a
// real branch on `p == 0` with the access in the fall-through and the constant as the cold tail:
// `cmplwi p,0; beq COLD; <access>; blr; COLD: li r3,CONST; blr`. The access may be `*p`, `p[const]`,
// or `p->field` (any load safe when p is non-null). A SAFE fall-through — a constant or another
// variable — folds to a branchless select instead (e.g. `if(!p) return 0; return b;` ->
// `neg;or;srawi;and`), which ours already matched.
//
// DEFERS (no wrong bytes): a char/short return (the cold constant would sign-extend, `li r0,0;
// extsb r3,r0`, where mwcc just `li r3,0`s), a dereference of a DIFFERENT pointer than the one guarded
// (`if(!p) return 0; return *q;`), and a variable index (`p[i]`, which needs the scaled register live).
struct S { int a, b; };
int      deref_or_zero (int *p)      { if (!p) return 0;  return *p; }     // cmplwi r3,0; beq 10; lwz r3,0(r3); blr; li r3,0; blr
int      deref_or_neg1 (int *p)      { if (!p) return -1; return *p; }     // ... ; li r3,-1; blr
unsigned uderef        (unsigned *p) { if (!p) return 0;  return *p; }     // same, lwz
int      mirror        (int *p)      { if (p)  return *p; return 0; }      // form B: same bytes
int      next_or_zero  (int *p)      { if (!p) return 0;  return p[1]; }   // p[const] tail: lwz r3,4(r3)
int      field_or_zero (struct S *p) { if (!p) return 0;  return p->b; }   // p->field tail: lwz r3,4(r3)
int      mirror_field  (struct S *p) { if (p)  return p->a; return -1; }   // form B member
