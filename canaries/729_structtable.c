// An array of word-field structs with a pointer field — a `{ ptr, id }` table —
// flattens to data relocations (pointer slots) interleaved with literal scalar
// bytes (the int fields). `&symbol`/`&global`/`0`/scalar elements have no
// anonymous @N object and are byte-exact; the `((void*)0)` NULL form parses. (A
// STRING element defers — its @N number is offset by phantom @N from header
// inlines in a real TU.)
extern int structtable_g;
typedef struct { int* p; int id; } StructTableEntry;
StructTableEntry structtable[3] = { {&structtable_g, 7}, {0, 3}, {((void*)0), -1} };
