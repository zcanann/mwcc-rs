// A `static const` ARRAY can't be folded into a register, so mwcc emits it to
// `.rodata` with a LOCAL symbol (unlike a static const SCALAR, which is folded
// into readers / elided when unused). Word, double, and float element types.
static const double scarr_d[2] = {1.0, 2.0};
static const int scarr_i[3] = {10, 20, 30};
static const float scarr_f[4] = {1.0f, 2.0f, 3.0f, 4.0f};
