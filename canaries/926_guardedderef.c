// A null-guarded dereference `if (!p) return CONST; return *p;` is now byte-exact for an int-width
// return. mwcc CANNOT fold it to a branchless `p ? *p : CONST` (dereferencing a null pointer is
// unsafe), so it emits a real branch with the DEREF in the fall-through and the constant as the cold
// tail: `cmplwi p,0; beq COLD; lwz r3,0(r3); blr; COLD: li r3,CONST; blr`. (A SAFE fall-through — a
// constant or another variable — folds to a branchless select instead, e.g. `if(!p) return 0;
// return b;` -> `neg;or;srawi;and`, which ours already matched.)
//
// DEFERS (no wrong bytes): a char/short return (the cold constant would sign-extend, `li r0,0;
// extsb r3,r0`, where mwcc just `li r3,0`s), a dereference of a DIFFERENT pointer than the one
// guarded (`if(!p) return 0; return *q;`), and a non-bare-deref tail (`p[1]`, `p->field`).
int      deref_or_zero (int *p)      { if (!p) return 0;  return *p; }  // cmplwi r3,0; beq 10; lwz r3,0(r3); blr; li r3,0; blr
int      deref_or_neg1 (int *p)      { if (!p) return -1; return *p; }  // ... ; li r3,-1; blr
unsigned uderef        (unsigned *p) { if (!p) return 0;  return *p; }  // same, lwz
