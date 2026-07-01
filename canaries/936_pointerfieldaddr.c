// `&p->field` where `p` is a register-resident struct POINTER is the pointer value plus the member
// offset — `mr dest,p` at offset 0, else `addi dest,p,offset` — the same shape as the MemberAddress
// value path. This is NOT the global-struct-VALUE `&g.field` address computation (materialize &g);
// here `p` already holds the address.
//
// SAFETY (no wrong bytes): the base register comes from general_register_of, which errors — so the
// whole address-of defers — when the base is not a register-resident integer/pointer. A frame-resident
// struct VALUE (`&s.field`) and a nested chain (`&q->pp->field`) therefore stay deferred, not wrong.
struct Pair  { int x, y; };
struct Big   { int a, b, c, d; };
struct Tag   { int id; char kind; };

int  *pair_y(struct Pair *p)  { return &p->y; }   // addi dest,p,4
int  *pair_x(struct Pair *p)  { return &p->x; }   // offset 0 -> mr (or nothing if p is already dest)
int  *big_d(struct Big *p)    { return &p->d; }   // addi dest,p,12
char *tag_kind(struct Tag *p) { return &p->kind; } // addi dest,p,4
int  *via_local(struct Pair *p) { struct Pair *q = p; return &q->y; } // local pointer var
