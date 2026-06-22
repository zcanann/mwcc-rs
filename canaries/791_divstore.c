// Signed division by a constant computed into a *store* (result register r0) was a
// miscompile: the sign-correction `srwi`/quotient overwrote r0 (the quotient or the
// to-be-shifted value), then `add r0,r0,r0` doubled the sign and lost the quotient.
// mwcc keeps the quotient in the freed dividend register and the sign in r0. (This was
// latent because the common case — `return a / k` — puts the result in r3, which
// happens to be the dividend register, so the collision didn't occur.)
int q2, q3, q6, q7, q8, q100;
void d2(int a)   { q2   = a / 2;   }   // pow2: srwi r0,r3,31; add r0,r0,r3; srawi
void d3(int a)   { q3   = a / 3;   }   // magic, no correction: mulhw r3,r0,r3; srwi r0,r3,31
void d6(int a)   { q6   = a / 6;   }   // magic, shift
void d7(int a)   { q7   = a / 7;   }   // magic, correction + shift
void d8(int a)   { q8   = a / 8;   }   // pow2 k>=2: srawi; addze
void d100(int a) { q100 = a / 100; }   // magic
// Signed modulo by 2^k into a store has the same latent collision — the sign lands in
// the dividend register x, not the result r0 (which holds the slwi value).
int r4, r8, r16;
void m4(int a)   { r4  = a % 4;  }   // slwi r0,r3,30; srwi r3,r3,31; subf; rotlwi; add r0,r0,r3
void m8(int a)   { r8  = a % 8;  }
void m16(int a)  { r16 = a % 16; }
