// A file-scope multi-declarator list where EACH declarator carries its own `*`
// (`int *a, *b;`) declares several pointer globals of the same type. `parse_type`
// consumes the first `*` into the base type, so a later declarator's leading `*`
// has to be consumed explicitly before its name — mirroring the local-declaration
// path. Both pointers land in .sbss (4 bytes each) and read/return byte-exactly.
//
// DEFERS (no wrong bytes): a MIXED list (`int a, *b;` — a later `*` with a
// non-pointer base) and a MULTI-LEVEL one (`int *a, **b;`) need a per-declarator
// type, so the parser reports them rather than mis-type a declarator.
int  *a, *b;
char *cp, *cq;

int  *first(void)   { return a; }   // read the first pointer global
int  *second(void)  { return b; }   // read the second (leading-`*`) pointer global
char *cfirst(void)  { return cp; }
char *csecond(void) { return cq; }
