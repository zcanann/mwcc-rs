// An array typedef (`typedef float Mtx[3][4];`) used as a struct member made the
// typedef registration choke on `[` and the member parse fail, skipping the whole
// struct — the dolphin Mtx member `Mtx unk_F0;` in ModelData. Record the typedef's
// element type and total length; a member of that type lays out as a flat element
// array (right size, so members after it land correctly). 1D element access works;
// a 2D matrix access defers in codegen rather than miscompiling.
typedef float Mtx[3][4];
typedef int   Quad[4];
struct Node {
    int  id;
    Mtx  transform;
    Quad corners;
    int  flags;
};
int  get_id(struct Node *n)    { return n->id; }
int  get_flags(struct Node *n) { return n->flags; }
int  get_corner(struct Node *n){ return n->corners[2]; }
