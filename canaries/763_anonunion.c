// An anonymous union member flattens its members into the enclosing struct, all
// sharing the union's offset (overlapping storage); the union occupies its largest
// member. Members may be scalars, pointers, struct values (which keep chaining via
// .member.field), or arrays. This is how the game's model structs overlay variant
// payloads. (Inline-struct union members are a deeper case and still defer.)
struct Vec { int x, y, z; };
struct Obj {
    char *name;                 /* 0x00 */
    int type;                   /* 0x04 */
    union {                     /* 0x08, size 12 (the Vec) */
        struct Vec data;
        unsigned short pair[2];
        int n;
    };
    int flags;                  /* 0x14 */
};
int read_type(struct Obj *p)  { return p->type; }
int read_flags(struct Obj *p) { return p->flags; }
int read_n(struct Obj *p)     { return p->n; }
