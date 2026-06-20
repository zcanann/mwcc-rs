// A pointer array global initialized with addresses: each element an ADDR32 data
// relocation. A small array (≤ 8 bytes) lands in .sdata, a larger one in .data.
// Within one object both the target SYMBOLS and the relocation ENTRIES run in
// reverse element order; a partly listed array zero-fills (null) the rest.
extern int ptrarray_a, ptrarray_b, ptrarray_c;
int* ptrarray_small[2] = { &ptrarray_a, &ptrarray_b };
int* ptrarray_large[3] = { &ptrarray_a, &ptrarray_b, &ptrarray_c };
