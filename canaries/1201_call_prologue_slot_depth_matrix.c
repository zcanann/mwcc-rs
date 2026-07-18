// Probe the complete ready-argument run around a non-leaf prologue. Mainline has two slots before
// the saved-LR store. Build 163 spans both linkage writes: one argument between mflr/store and two
// between store/stwu; a fourth stays in the body. Shifted incoming parameters also establish that
// build 163 encodes dying downward argument moves as addi-zero, unlike a duplicated r3->r4 value.

void slot4(int, int, int, int);

void four_constants(void) { slot4(1, 2, 3, 4); }

void four_shifted_parameters(int a, int b, int c, int d, int e)
{
    slot4(b, c, d, e);
}
