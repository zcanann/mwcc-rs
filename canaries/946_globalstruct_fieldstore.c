// A SINGLE field store to a global struct VALUE is byte-exact: an offset-0 field via a direct SDA21
// store (`stw v, g@sda21`), a non-zero-offset field by materializing the struct's SDA base and
// storing at the displacement (`li rB,g@sda21; stw v, off(rB)`).
//
// MULTIPLE field stores to the SAME global struct (`g.x=..; g.y=..;`) DEFER (no wrong bytes): mwcc
// materializes the shared SDA base once and schedules it AHEAD of the first field store, then
// addresses each field from it — a shared-base schedule our per-store, program-order materialization
// does not yet reproduce. Two DIFFERENT global structs, or a pointer's fields, are unaffected.
struct P { int x, y; };
struct P g;
struct P h;
void set_x(int v)     { g.x = v; }   // stw v, g@sda21
void set_y(int v)     { g.y = v; }   // li rB,g@sda21; stw v, 4(rB)
void set_other(int v) { h.x = v; }   // a different struct's field
