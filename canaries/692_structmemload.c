// Reading a field of a frame-resident struct local after its address escapes:
// `g(&v)` forces `v` into a stack slot, then `return v.y` is a displacement load
// from that slot plus the member offset (`lwz r3, slot+4(r1)`). The matching
// field STORE is deferred for now — before a call mwcc's scheduler materializes
// the call-argument address among the field stores, which this path can't yet
// reproduce — but the post-call LOAD has no such ordering hazard.
struct B { int x; int y; };
void g(struct B *);
int structmemload(void){ struct B v; g(&v); return v.y; }
