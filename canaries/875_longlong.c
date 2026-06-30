// First increment of long long (64-bit) support. A long long lives in a general-register PAIR,
// big-endian: r3:r4 is high:low. The parser now recognizes `long long` / `unsigned long long` as
// distinct 64-bit types, and a dedicated handler emits the modeled shapes (everything else — long
// long parameters, locals, arithmetic, loads — defers, never falling through to the 32-bit path).
// Modeled here: a small integer CONSTANT return (`li r4,LOW; li r3,HIGH`, low word first), and a
// 32-bit int/unsigned parameter WIDENED to long long (`mr r4,r3; srawi r3,r3,31` for signed,
// `mr r4,r3; li r3,0` for unsigned). A narrow (short/char) source re-extends differently and defers.
long long          ret0(void)             { return 0; }
long long          ret5(void)             { return 5; }
long long          retneg(void)           { return -1; }
long long          widen(int a)           { return a; }
unsigned long long uwiden(unsigned a)     { return a; }
