// A large (> 8 byte) writable global lands in `.data` (initialized) — past the
// 8-byte small-data threshold, so not `.sdata`. The section sits after `.rodata`
// and before the small-data sections; the symbol is a GLOBAL object.
int largedata[4] = { 1, 2, 3, 4 };
