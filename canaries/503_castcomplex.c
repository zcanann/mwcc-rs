// A narrowing cast over a both-complex value: the operand computes into the
// scratch (its temporary a virtual), then extsh/clrlwi narrows — the same gate
// the register allocator relaxed, now reaching the cast path.
short castcomplex(int a, int b, int c, int d){ return (short)((a + b) * (c + d)); }
