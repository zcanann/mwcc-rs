// A store to element 0 of a large (ADDR16) file-scope array folds the low half of
// the address into the store displacement — `lis base,a@ha; stw v,a@l(base)` — rather
// than materializing the whole base with an `addi` first. A non-zero element offset
// keeps the `addi` (the literal offset rides the store displacement instead).
int  gas_words[4];
short gas_halfs[8];
char gas_bytes[16];
void gas_store0_word(int v)   { gas_words[0] = v; }
void gas_store1_word(int v)   { gas_words[1] = v; }
void gas_store0_half(short v) { gas_halfs[0] = v; }
void gas_store0_byte(char v)  { gas_bytes[0] = v; }
