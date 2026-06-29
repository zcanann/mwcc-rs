// A select against zero where the non-zero arm is a single-op COMPUTED value —
// `c ? 0 : a + 1` / `c ? a * 2 : 0` (and the guard form `if (c) return 0; return a+1;`).
// mwcc builds the all-ones-when-nonzero mask `srawi(-c|c, 31)` and combines it with the
// value via and/andc — for a computed arm the value is evaluated into the scratch and the
// `-c` temp goes in a fresh register the allocator places after the value's operands:
//
//     c ? 0 : a+1  ->  neg r5,c ; addi r0,a,1 ; or r3,r5,c ; srawi r3,r3,31 ; andc r3,r0,r3
//     c ? a*2 : 0  ->  neg r5,c ; slwi r0,a,1 ; or r3,r5,c ; srawi r3,r3,31 ; and  r3,r0,r3
//
// (and/andc by which arm is the constant 0). A leaf or constant arm was already handled;
// a multi-op value defers. The condition is a truthy leaf.
int sel_zero_add(int a, int c)   { return c ? 0 : a + 1; }   // andc
int sel_mul_zero(int a, int c)   { return c ? a * 2 : 0; }   // and
int sel_zero_mask(int a, int c)  { return c ? 0 : a & 0xff; }
int sel_zero_neg(int a, int c)   { return c ? 0 : -a; }
int guard_zero_add(int a, int c) { if (c) return 0; return a + 1; }  // same via the guard

// The same select-against-zero with a value-tracked LOCAL feeding the guarded return —
// `int x = a+1; if (c) return 0; return x;`. mwcc materializes the local in its result
// home but SCHEDULES the materialization into the select's neg->or latency slot:
//
//     neg r0,c ; addi r3,a,1 ; or r0,r0,c ; srawi r0,31 ; andc r3,r3,r0   (return x;  arm 0 = then)
//     neg r0,c ; addi r3,a,1 ; or r0,r0,c ; srawi r0,31 ; and  r3,r3,r0   (return 0;  arm 0 = else)
//
// The guard dispatch emits that interleave directly (leading neg, the local, then the mask
// combine). Restricted to a single-op integer local, a leaf condition, and one arm 0.
int guard_local_then(int a, int c)  { int x = a + 1; if (c) return 0; return x; }   // andc
int guard_local_else(int a, int c)  { int x = a * 2; if (c) return x; return 0; }   // and
int guard_local_mask(int a, int c)  { int x = a & 0xff; if (c) return 0; return x; }
