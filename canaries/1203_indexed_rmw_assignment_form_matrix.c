// Build 163 preserves a frontend distinction between compound assignment and
// an explicitly spelled read/modify/write.  Keep the pairs adjacent so the
// address shape can be compared without conflating it with the arithmetic op.
void compound_add(int *a, int i) { a[i] += 3; }
void explicit_add(int *a, int i) { a[i] = a[i] + 3; }
void compound_sub(int *a, int i) { a[i] -= 3; }
void explicit_sub(int *a, int i) { a[i] = a[i] - 3; }
void compound_or(int *a, int i) { a[i] |= 7; }
void explicit_or(int *a, int i) { a[i] = a[i] | 7; }
void compound_xor(int *a, int i) { a[i] ^= 7; }
void explicit_xor(int *a, int i) { a[i] = a[i] ^ 7; }
void compound_and(int *a, int i) { a[i] &= 0x7fff; }
void explicit_and(int *a, int i) { a[i] = a[i] & 0x7fff; }
void compound_mul(int *a, int i) { a[i] *= 9; }
void explicit_mul(int *a, int i) { a[i] = a[i] * 9; }
void compound_add_leaf(int *a, int i, int x) { a[i] += x; }
void explicit_add_leaf(int *a, int i, int x) { a[i] = a[i] + x; }
void compound_or_leaf(int *a, int i, int x) { a[i] |= x; }
void explicit_or_leaf(int *a, int i, int x) { a[i] = a[i] | x; }
