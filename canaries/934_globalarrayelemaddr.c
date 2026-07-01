// `&g[i]` for a file-scope ARRAY global is the element ADDRESS `&g + i*sizeof(element)` — an address
// computation (`lis;addi;addi` for a large `.data` array via ADDR16, `addi;addi` for a small `.sdata`
// one via SDA21), NOT the pointer arithmetic `load(g)+i`. `g` is an array, so loading it (as if it
// were a pointer) and adding the unscaled index would be wrong bytes. The base materializes like a
// global-array subscript; the constant index folds to a scaled `addi` (or nothing at offset 0).
//
// (Regression guard: this whole shape previously emitted `lwz g; addi r3,r3,i` — a load of the array
// plus the raw index — which was a DIFF against mwcc's `lis;addi;addi`.)
//
// DEFERS (no wrong bytes): a variable index (`&g[i]`) — a runtime scale+add of an address is not
// modeled yet. A pointer `&p[i]` still uses ordinary pointer arithmetic and is unaffected.
int   words[10];              // large -> ADDR16
char  bytes[100];             // large -> ADDR16
int   pair[2];                // small (8 bytes) -> SDA21

int  *word_three(void)  { return &words[3]; }   // lis;addi;addi r3,r3,12
int  *word_zero(void)   { return &words[0]; }   // just &words (offset 0, no add)
char *byte_five(void)   { return &bytes[5]; }   // lis;addi;addi r3,r3,5
int  *pair_one(void)    { return &pair[1]; }    // SDA21 addi;addi r3,r3,4
