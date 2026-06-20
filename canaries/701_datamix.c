// One unit exercising every writable/read-only data section and the order
// `.rodata` < `.data` < `.bss` < `.sdata` < `.sbss` < `.sdata2`: a small init
// global (.sdata), a const array (.rodata), a small const scalar (.sdata2), a
// large init array (.data), and a large zero array (.bss).
int datamix_small = 1;
const int datamix_ro[4] = { 5, 6, 7, 8 };
const int datamix_k = 9;
int datamix_big[4] = { 1, 2, 3, 4 };
int datamix_zero[6];
