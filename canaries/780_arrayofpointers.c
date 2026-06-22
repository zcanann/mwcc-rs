// An array-of-pointers member (`u8 *mess_stack[8];` in game's WindowData) has no
// scalar pointee, so pointee_of errored and skipped the whole struct. Like the
// struct-value-array case, its element size (4 bytes per pointer) still lays the
// array out correctly, so members after it land at the right offset. Indexed element
// access defers in codegen rather than miscomputing.
struct Win {
    int    id;
    char  *stack[8];
    struct Win *links[4];
    int    flags;
};
int get_id(struct Win *w)    { return w->id; }
int get_flags(struct Win *w) { return w->flags; }
