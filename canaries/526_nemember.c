// Comparison-to-bool, != with a non-leaf operand. Unlike </> (which keep the
// twice-used operand in a preserved register off the destination), the !=
// idiom's two uses are the adjacent subtractions with nothing competing for the
// scratch between them, so mwcc evaluates the non-leaf straight into the scratch
// (r0) and lets the second subtraction overwrite it.
struct S { int a; };
int nemember(struct S* p, int x){ return x != p->a; }
