// The offset-0 element of a SMALL (SDA21-addressed, <= 8 byte) global array folds to a SINGLE
// direct SDA21 load — `lwz d, g@sda21(r0)` — exactly like a scalar global; mwcc does not
// materialize the array base for `g[0]`. A NON-zero element offset cannot fold (an SDA21
// relocation carries no addend), so it materializes the base (`li d,g@sda21; lwz d,off(d)`); a
// LARGE array is ADDR16 and always materializes the base (`lis;addi;lwz`).
int   wi[2];      // 8-byte int array   -> SDA21
short hs[2];      // 4-byte short array -> SDA21

int  first_word(void)  { return wi[0]; }   // lwz r3, wi@sda21(r0)          (folded)
int  second_word(void) { return wi[1]; }   // li r3, wi@sda21; lwz r3,4(r3) (base + displacement)
int  first_half(void)  { return hs[0]; }   // lha r3, hs@sda21(r0)          (folded)
