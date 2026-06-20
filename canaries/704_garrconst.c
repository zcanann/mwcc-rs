// Constant-index access to a file-scope array global: materialize the array's
// base address by size (SDA21 for a small `.sdata` array, `lis`/`addi` ADDR16 for
// a large `.data` one), then a displacement load at the element offset.
int garrconst_small[2] = { 10, 20 };
static int garrconst_large[4] = { 1, 2, 3, 4 };
int garrconst_s(void) { return garrconst_small[1]; }
int garrconst_l(void) { return garrconst_large[2]; }
