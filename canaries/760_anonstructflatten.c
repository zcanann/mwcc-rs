// An anonymous struct member promotes (flattens) its fields into the enclosing
// struct (C anonymous-struct semantics) — including bit-field groups, the way the
// board's PlayerState-style structs wrap them. Layout (size, member offsets) stays
// byte-exact; flattened non-bit members resolve to their offsets, and a non-bit
// member after an anonymous bit-field struct lands at the right place.
struct Nested {
    struct { int a, b; };
    int c;
};
struct WithBf {
    struct { unsigned short col:2, moving:1; };
    int n;
    struct { unsigned char team:1, spark:1; };
};
struct Nested anf_n;
struct WithBf anf_w;
int anf_first(struct Nested *p)  { return p->a; }
int anf_flat(struct Nested *p)   { return p->c; }
int anf_after(struct WithBf *p)  { return p->n; }
