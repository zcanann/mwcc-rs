// An inline function-pointer struct member `RET (*name)(params);` is a 4-byte
// pointer for layout (the return type is parsed but irrelevant). parse_struct_body
// previously expected an identifier after the type and skipped the whole struct on
// the `(*...)` declarator; now it consumes the declarator and records a pointer
// member, so layout and `p->name` resolve. (The typedef'd form already worked.)
struct Ops {
    int kind;
    int  (*compare)(int, int);
    void (*reset)(void);
    int  count;
};
int  get_kind(struct Ops *p)  { return p->kind; }
int  get_count(struct Ops *p) { return p->count; }
