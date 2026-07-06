// flags: -char unsigned
// With `-char unsigned` (the build default on GC/1.3 build 53) a plain `char` is UNSIGNED: a read is a
// zero-extending `lbz`/`lhz` with NO `extsb`, and a narrow value still PROMOTES to a signed int before a
// `>>` so the shift stays the arithmetic `srawi`. The parser maps plain `char` -> Type::UnsignedChar when
// char_is_signed is false (config.char_is_signed()); `signed char` stays signed. This canary pins the
// flag so the char-unsigned path is regression-tested even though the gate build (1.3.2) is char-signed. (fire 611)
char gc;
int  cderef(char* s)   { return *s; }        // lbz r3,0(r3); blr        (no extsb — unsigned)
int  cidx(char* s)     { return s[2]; }      // lbz r3,2(r3); blr
void cadd(void)        { gc += 1; }          // lbz r0; addi r0,r0,1; stb r0   (no extsb)
void cand(void)        { gc &= 0xf; }        // lbz r0; clrlwi; stb
void cshr(void)        { gc >>= 1; }          // lbz r0; srawi r0,r0,1; stb    (promotes to signed int)
