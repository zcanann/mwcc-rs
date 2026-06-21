// Member access offset-folding. (1) A nested member through an EMBEDDED struct
// value accumulates offsets rather than dereferencing the sub-struct: `p->s.b`
// is `lwz d, off(p)`, one load, not two. (2) An offset-0 member of a small,
// SDA-addressed global struct folds to a single SDA21 load `lwz d, g@sda21`,
// like a scalar global (signed-char members re-extend); larger structs and
// non-zero offsets materialize the base.
struct MembersFoldS { int a; int b; };
struct MembersFoldT { int head; struct MembersFoldS s; };
struct MembersFoldS membersfold_g;
int membersfold_nested(struct MembersFoldT *p) { return p->s.b; }
int membersfold_global0(void) { return membersfold_g.a; }
int membersfold_global4(void) { return membersfold_g.b; }
