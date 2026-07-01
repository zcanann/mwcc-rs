// An indexed read-modify-write by a power-of-two multiplier strength-reduces to a left
// shift, like every other multiply context: `a[i] *= 2` -> `slwi r0,r0,1` (NOT
// `mulli r0,r0,2`). A non-power-of-two keeps `mulli`. Previously try_emit_indexed_rmw
// always emitted `mulli` for `a[i] *= C`, a byte-DIFF for power-of-two constants.
void mul2(int *a, int i)  { a[i] *= 2; }        // slwi 1
void mul4(int *a, int i)  { a[i] *= 4; }        // slwi 2
void mul16(int *a, int i) { a[i] *= 16; }       // slwi 4
void mul3(int *a, int i)  { a[i] *= 3; }        // mulli 3 (non-pow2)
void mul9(int *a, int i)  { a[i] = a[i] * 9; }  // mulli 9 (explicit form)
