// mwcc keeps a constant-amount shift as the FIRST operand of a commutative op (`(a<<2)+b` ->
// `slwi r0,r3,2; add r3,r0,r4`, shift first), but our placement swaps it (like the strength-reduced
// `(a*4)+b`, where mwcc DOES put the leaf first). So a commutative op (+,|,&,^,*) with a const-shift
// LEFT and a NON-CONSTANT right DEFERS (was swapped bytes). These stay byte-exact: a const-shift on
// the RIGHT, a non-commutative op, a VARIABLE shift, and a CONSTANT right that FUSES to rlwinm.
int      sh_right(int a, int b)     { return b + (a << 2); }       // shift on right -> source order
int      sh_sub(int a, int b)       { return (a << 2) - b; }       // non-commutative subtract
int      sh_var(int a, int b, int c){ return (a << c) + b; }       // variable shift amount
unsigned sh_fuse(unsigned x)        { return (x >> 16) & 0x7fff; } // constant mask -> single rlwinm
