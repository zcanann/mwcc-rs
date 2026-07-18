// Build 163's redundant-cast pass only removes a signed narrowing cast before
// a same-width store for low-bit-preserving binary ALU expressions. Scalar and
// loaded values, shifts, and unary operations retain extsb/extsh; the 2.4.x
// mainline recognizes the final stb/sth already truncates and removes them.
int gi;
short gs;

void cast_global(void)       { gs = (short)gi; }
void cast_deref(int* p)      { gs = (short)*p; }
void cast_shift_left(int a)  { gs = (short)(a << 2); }
void cast_shift_right(int a) { gs = (short)(a >> 2); }
void cast_negate(int a)      { gs = (short)-a; }

// Contrast: this cast is eliminated in both generations.
void cast_add(int a, int b)  { gs = (short)(a + b); }
