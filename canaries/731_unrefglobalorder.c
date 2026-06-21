// Unreferenced (.sbss/.bss) globals trail the functions in the symbol table in
// REVERSE declaration order (mwcc: `int a;b;c;` -> symbols `c b a`). A referenced
// global appears up front in reference order; the rest reverse.
int urg_a;
int urg_b;
int urg_c;
int urg_get(void) { return urg_b; }
