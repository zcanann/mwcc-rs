// Same-type bit-fields pack MSB-first into sizeof(type) storage units; a field that
// overflows the unit starts a new one, and an ordinary member after them aligns
// normally. The struct registers with a byte-exact layout (size drives the global's
// reserved bytes; a non-bit member resolves to its correct offset). Bit-field access
// itself defers until extract/insert codegen.
struct BfWord  { unsigned short a:2, b:1, c:4, d:2, e:1, f:6; };  /* 16 bits -> 2 */
struct BfOver  { unsigned short a:8, b:8, c:1; };                  /* 17 bits -> 4 */
struct BfBytes { unsigned char a:1, b:1, c:2; };                   /* -> 1 */
struct BfThenN { unsigned short a:2, b:1; int n; };               /* -> 8, n@4 */
struct BfWord  bf_w;
struct BfOver  bf_o;
struct BfBytes bf_b;
struct BfThenN bf_t;
int bf_after(struct BfThenN *p) { return p->n; }
