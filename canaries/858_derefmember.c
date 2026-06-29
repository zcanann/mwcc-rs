// `(*p).field` is exactly `p->field` — dereference-then-member is the arrow access. The parser
// resolved the member tag for the arrow and cast forms but not for a Dereference base, so
// `(*p).a` deferred ("member on a non-struct-pointer base"). Now the tag comes from the
// pointer (a struct/union-pointer variable, or a recorded cast tag) and the member's base is
// unwrapped one deref level — so it lowers identically to `p->a` (`lwz r3,0(r3)`), not a
// spurious double load. Covers struct and union, load and store. The CW GX FIFO macro
// `(*(volatile PPCWGPipe*)ADDR).u8 = v` is this same shape over a constant-address cast.
struct S { int a; int b; };
int  ld_a(struct S *p)            { return (*p).a; }   // == p->a
int  ld_b(struct S *p)            { return (*p).b; }   // offset member
void st_a(struct S *p, int v)     { (*p).a = v; }      // store
union U { unsigned char b; int w; };
int  uld(union U *p)              { return (*p).b; }    // union load
void ust(union U *p, int v)       { (*p).w = v; }       // union store
