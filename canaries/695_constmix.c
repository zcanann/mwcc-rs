// All three data sections in one unit: a writable scalar in `.sdata`, a large
// const array in `.rodata`, and a small const scalar in `.sdata2`. Exercises the
// section order (`.rodata` < `.sdata` < `.sdata2`) and the GLOBAL-symbol run,
// which emits the initialized-section objects in declaration order.
int constmix_w = 1;
const int constmix_ro[4] = { 1, 2, 3, 4 };
const int constmix_k = 5;
