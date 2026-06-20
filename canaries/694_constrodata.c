// A larger `const` global (> 8 bytes) lands in `.rodata` — a read-only
// (ALLOC-only, no WRITE) section ordered right after the unwind tables and ahead
// of the writable small-data sections. An array object is word-aligned (4).
const int constrodata[4] = { 1, 2, 3, 4 };
