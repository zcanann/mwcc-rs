// A SIGNED char dereference `*p` used as a truthiness value `(*p) != 0` must be sign-extended
// before the bool idiom, like mwcc: `lbz r0,0(r3); extsb r3,r0; neg r0,r3; or r0,r0,r3;
// srwi r3,r0,31`. The `!= 0` idiom in emit_comparison (comparisons.rs) loaded the byte with a bare
// `lbz` into the result and ran the idiom without the extsb — a byte diff (the `== 0` leading-zero
// case already extended; the truthiness path skipped it). Now `lbz` into the scratch, `extsb` into
// the destination, then the idiom. Only signed char is affected: unsigned char, short, ushort, int,
// and the plain `return *p` value were already byte-exact. (Guarded to a real-register destination;
// the scratch/store destination still defers, not DIFFs.) Found while scoping data-dependent loops,
// where `while(*p)` would inherit this.
int deref_select(char *p) { return *p ? 1 : 0; }
int deref_guard(char *p)  { if (*p) return 1; return 0; }
