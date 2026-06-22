// A struct-value array member (`GXTexRegion TexRegions[8];` in dolphin's GXData)
// made parse_struct_body call pointee_of on a struct element, which errored and
// skipped the whole struct. Skip pointee_of for struct elements — the element tag
// is in struct_tag and the size still lays out correctly, so members AFTER the
// array land at the right offset. (Indexed element access still defers in codegen.)
typedef struct { int x, y; } Pt;
struct Mesh {
    int      count;
    Pt       verts[4];
    int      flags;
};
int get_count(struct Mesh *m) { return m->count; }
int get_flags(struct Mesh *m) { return m->flags; }
