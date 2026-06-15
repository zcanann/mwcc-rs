// marioparty4 MSL_C rand.c (includes reduced to the u32 typedef) — the FIRST real
// reference translation unit matched whole-object byte-exact. Exercises: typedef,
// an initialized .sdata global, multi-function, the global large-const multiply
// (allocator), value reuse of a just-stored global, and (x>>16)&0x7FFF -> rlwinm.
typedef unsigned long u32;
u32 next = 1;
u32 rand(void) {
	next = 0x41C64E6D * next + 12345;
	return (next >> 16) & 0x7FFF;
}
void srand(u32 seed) { next = seed; }
