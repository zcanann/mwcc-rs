// A typedef'd union resolves to its struct-like layout (every member at offset 0), exactly as a
// typedef'd struct does, so `p->member` is a plain load/store at offset 0. The parser previously
// only routed `typedef struct {…} Alias;` through the layout-registering path; `typedef union {…}
// Alias;` fell through to parse_type, which cannot read an inline union body, so member access on
// the alias deferred. Both the anonymous and tagged forms now register the union under its tag and
// map the alias to it. This is the dolphin register-union shape (`typedef union { u32 val; struct
// { ... } bits; } REG;`). A union VALUE parameter rides along (treated like a struct value).
typedef union { int i; float f; } UIF;
typedef union Named { int i; float f; } UNamed;
typedef union { unsigned hi; struct { short a, b; } bits; } UReg;
int   geti(UIF *p)            { return p->i; }
int   getnamed(UNamed *p)     { return p->i; }
void  seti(UIF *p, int v)     { p->i = v; }
float getf(UIF *p)            { return p->f; }
int   getbits(UReg *p)        { return p->bits.a; }
