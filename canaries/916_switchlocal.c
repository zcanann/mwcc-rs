// A value-tracked local that only feeds a switch's scrutinee inlines into the switch: mwcc compiles
// `int m = n + 1; switch(m)` identically to the direct `switch(n + 1)` (addi r0,r3,1; cmpwi r0,1; ...).
// A pass (inline_switch_scrutinee_locals) substitutes the local into the scrutinee/arms and recompiles.
// Gated to int-width, call-free, read-AT-MOST-ONCE locals; a narrow local, a multiply-read local (mwcc
// keeps a computed value in a register), or a call-initialized local defers honestly — never wrong bytes.
int      add1(int n)         { int m = n + 1; switch(m){ case 0: return 1; case 1: return 2; default: return 0; } }
int      mask(int n)         { int m = n & 3; switch(m){ case 0: return 1; case 1: return 2; case 2: return 3; default: return 4; } }
int      sum(int a, int b)   { int m = a + b; switch(m){ case 0: return 1; case 1: return 2; default: return 0; } }
unsigned umask(unsigned n)   { unsigned m = n & 3; switch(m){ case 0: return 1; case 1: return 2; case 2: return 3; default: return 4; } }
