// Negating a both-complex expression: its inner temporary is now a virtual the
// allocator places, so the operand evaluates into the scratch like mwcc
// (mullw r0,...; neg r3,r0) — a deferral relaxed once the allocator owns temps.
int negcomplex(int a, int b, int c, int d){ return -((a + b) * (c + d)); }
