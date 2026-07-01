// `sizeof` of a file-scope variable folds to a compile-time `size_t` constant (`li r3,N`), the same
// as `sizeof(type)` and `sizeof(local)`. A global's total byte size answers `sizeof(g)`; its array
// element size answers `sizeof(g[0])`, so the classic element-count idiom `sizeof(a)/sizeof(a[0])`
// folds end to end. The parser records both sizes when the global is declared (they are NOT cleared
// per function, unlike the local `sizeof` maps), and a local of the same name still shadows the global.
//
// SAFETY (no wrong bytes): the element size is recorded only for an ARRAY global — a pointer global's
// `sizeof(*p)` / `sizeof(p[0])` wants the pointee, not the 4-byte pointer, so it stays deferred rather
// than folding to a wrong value.
int           counts[10];
char          buffer[256];
int           scalar;
struct Pair   { int x, y; };
struct Pair   pairs[4];

int total_bytes(void)   { return sizeof(counts); }                 // 10 * 4  = 40
int element_count(void) { return sizeof(counts) / sizeof(counts[0]); } // 40 / 4  = 10
int buffer_size(void)   { return sizeof(buffer); }                 // 256
int scalar_size(void)   { return sizeof(scalar); }                 // 4
int pairs_bytes(void)   { return sizeof(pairs); }                  // 4 * 8   = 32
int pairs_count(void)   { return sizeof(pairs) / sizeof(pairs[0]); }   // 32 / 8  = 4
