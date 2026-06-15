// Right-shift of a both-complex value: the value computes into the scratch (its
// temporary a virtual), then srawi/srwi shifts — the place_operand_or_scratch
// family reaching the shift path.
int shrcomplex(int a, int b, int c, int d){ return ((a + b) * (c + d)) >> 3; }
