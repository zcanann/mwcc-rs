// A pointer global initialized with the address of another symbol becomes four
// zero bytes in .sdata plus an ADDR32 data relocation to that symbol. mwcc emits
// each pointer's relocation target symbol immediately after the pointer (`p, &a;
// q, &b`). A null pointer has no relocation and stays in .sbss.
extern int ptrreloc_a;
extern int ptrreloc_b;
int* ptrreloc_p = &ptrreloc_a;
int* ptrreloc_q = &ptrreloc_b;
int* ptrreloc_null = 0;
