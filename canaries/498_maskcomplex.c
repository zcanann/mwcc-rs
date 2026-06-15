// An immediate fold over a both-complex operand — the same gate relaxation.
int maskcomplex(int a, int b, int c, int d){ return ((a + b) * (c + d)) & 255; }
