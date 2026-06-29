// A truncation-safe op on a signed-char (or short) GLOBAL whose result stores back to a
// NARROW target re-truncates through the store (`stb`/`sth`), so mwcc reads the global RAW —
// `char gc; gc += 1;` is `lbz r3; addi r0,r3,1; stb r0`, NOT `lbz; extsb; addi; stb`. The
// byte store drops the high bits the extsb would have sign-extended, so it is redundant.
// emit_global_load now skips the extsb under narrow_truncation_context, and place_store_value
// sets that flag for a `var op const` narrow store (Add/Sub/Or/Xor/Mul/And — div/mod and
// shift-right keep the extsb since the sign matters; shift-left was already exact). The same
// flag makes a narrow RETURN of a global byte-exact (`char f(){ return gc + 1; }`).
char  gc;
short gs;
void cadd(void)  { gc += 1; }    // lbz r3; addi r0,r3,1; stb r0   (no extsb)
void csub(void)  { gc -= 2; }
void cmul(void)  { gc *= 3; }
void cor(void)   { gc |= 4; }
void cand(void)  { gc &= 0xf; }  // lbz r0; clrlwi; stb            (no extsb)
void cxor(void)  { gc ^= 1; }
void cinc(void)  { gc++; }
void cshr(void)  { gc >>= 1; }   // KEEPS the extsb (arithmetic shift needs the sign)
void sadd(void)  { gs += 1; }
char cret(void)  { return gc + 1; }  // narrow return of a global: also raw-read, byte-exact
