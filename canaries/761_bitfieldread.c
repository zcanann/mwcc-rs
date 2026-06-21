// An unsigned bit-field read is the smallest unit load (byte/halfword/word covering
// the field's bytes) shifted and masked to the field's bits — `(load >> shift) &
// mask` — which lowers to mwcc's `lbz/lhz; rlwinm`. Works through a struct pointer,
// including a field promoted from an anonymous struct.
struct Bf { unsigned short a:2, b:1, c:4, d:8; };
int bfr_a(struct Bf *p) { return p->a; }
int bfr_b(struct Bf *p) { return p->b; }
int bfr_c(struct Bf *p) { return p->c; }
struct Wide { unsigned a:5, b:7, c:20; };
int bfr_wide(struct Wide *p) { return p->b; }
struct Bytes { unsigned char x:1, y:1, z:2; };
int bfr_byte(struct Bytes *p) { return p->z; }
struct Anon { struct { unsigned short col:2, moving:1; }; int n; };
int bfr_anon(struct Anon *p) { return p->col; }
