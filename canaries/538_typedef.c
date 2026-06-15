// typedef + `unsigned long` parsing. The alias resolves to its underlying type,
// so codegen is identical to using the type directly.
typedef unsigned long u32;
u32 idu32(u32 a){ return a; }
