// A local COMPUTED from parameters that is live across a call — passed to it and
// returned — is preserved in r31: `int z = x + 1; g(z); return z;` becomes
// `addi r31,r3,1; mr r3,r31; bl g; ... mr r3,r31`. The argument calls may pass only
// z and constants; a parameter or global argument (call-clobbered) defers.
extern void clac_use(int);
extern void clac_more(int);
int clac_one(int x)  { int z = x + 1; clac_use(z); return z; }
int clac_post(int x) { int z = x * 2; clac_use(z); return z + 1; }
int clac_two(int x)  { int z = x & 7; clac_use(z); clac_more(z); return z; }
