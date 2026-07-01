// The store counterpart of 954: writing the offset-0 element of a SMALL (SDA21-addressed,
// <= 8 byte) global array folds to a SINGLE direct SDA21 store — `stw v, g@sda21(r0)` — like a
// scalar global, with no base register materialized. A NON-zero element offset materializes the
// base (`li b,g@sda21; stw v,off(b)`); a LARGE array is ADDR16 (`lis b,g@ha; stw v,g@l(b)`).
int   wi[2];      // 8-byte int array   -> SDA21
short hs[2];      // 4-byte short array -> SDA21
char  cb[4];      // 4-byte char array  -> SDA21

void store_word_0(int x)    { wi[0] = x; }   // stw  r3, wi@sda21(r0)          (folded)
void store_word_1(int x)    { wi[1] = x; }   // li r4,wi@sda21; stw r3,4(r4)   (base + displacement)
void store_half_0(short x)  { hs[0] = x; }   // sth  r3, hs@sda21(r0)          (folded)
void store_byte_0(char x)   { cb[0] = x; }   // stb  r3, cb@sda21(r0)          (folded)
