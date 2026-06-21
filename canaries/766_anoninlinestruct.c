// An anonymous inline struct can be a *named* member of a struct or a union
// variant (`struct { … } mesh;`). Its layout registers under a synthetic tag so
// `parent.mesh.field` chains, and it lays out as an ordinary struct value (in a
// union, at offset 0). This is the shape of the game's model structs.
struct InStruct {
    int head;                       /* 0x00 */
    struct { int x, y; } mesh;      /* 0x04 (8 bytes) */
    int tail;                       /* 0x0C */
};
struct InUnion {
    int head;                                   /* 0x00 */
    union { struct { int x, y, z; } big; int n; }; /* 0x04 (12 bytes) */
    int tail;                                   /* 0x10 */
};
int s_tail(struct InStruct *p) { return p->tail; }
int s_mesh(struct InStruct *p) { return p->mesh.y; }
int u_tail(struct InUnion *p)  { return p->tail; }
int u_big(struct InUnion *p)   { return p->big.z; }
