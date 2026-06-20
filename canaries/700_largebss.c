// A large (> 8 byte) zero/uninitialized writable global lands in `.bss` (NOBITS:
// a size, no file bytes) — the large-data counterpart to `.sbss`.
int largebss[6];
