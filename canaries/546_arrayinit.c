// MSL_C float.c (reduced): array globals with aggregate initializers. They land
// in .sdata in FORWARD declaration order (nan@0, huge@4) — unlike .sbss which is
// reverse. `[]` infers the length from the initializer.
unsigned long __float_nan[] = { 0x7FFFFFFF };
unsigned long __float_huge[] = { 0x7F800000 };
