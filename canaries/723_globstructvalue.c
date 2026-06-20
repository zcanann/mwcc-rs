// Reading a member of a GLOBAL struct VALUE: materialize the struct's address
// (SDA21 `li d,g@sda21` when small, `lis;addi` when large) then load the field
// at its offset. The struct global occupies struct_size bytes at the struct's
// alignment, floored at a word (4) — a fix to the data object, which previously
// used the word-default scalar width (8-byte struct emitted as 4 bytes).
struct GsvWord { int first; int second; };
struct GsvChar { char a; char b; };
struct GsvBig  { int a; int b; int c; };
struct GsvWord gsv_word;
struct GsvChar gsv_char;
struct GsvBig  gsv_big;
int gsv_get_word(void) { return gsv_word.second; }
int gsv_get_char(void) { return gsv_char.b; }
int gsv_get_big(void)  { return gsv_big.c; }
