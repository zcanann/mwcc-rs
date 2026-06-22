// A float comparison whose value is a struct member or file-scope global (not a
// register leaf) loads that value with `lfs`/`lfd` while the pool constant is in the
// other register. Ordered (`>`,`<=`): const in f0, value in f1, `fcmpo f1,f0`. The
// `==`/`!=` form uses mwcc's *swapped* assignment — const in f1 (loaded first), value
// in f0, `fcmpu f1,f0`. The integer literal promotes to the comparison's precision.
// (A member compared with a float argument already in f1 still needs the FP allocator,
// deferred not miscompiled.)
struct Body { int flags; float speed; double mass; };
void tick(void);
void a(struct Body* b) { if (b->speed > 0.0f)  tick(); }
void c(struct Body* b) { if (b->speed > 0)     tick(); }
void d(struct Body* b) { if (b->speed <= 1.0f) tick(); }
void e(struct Body* b) { if (b->mass > 0.0)    tick(); }
void eq(struct Body* b){ if (b->speed == 0.0f) tick(); }   // swapped: lfs f1,k; lfs f0,(v); fcmpu f1,f0
void ne(struct Body* b){ if (b->mass != 0.0)   tick(); }   // double != via lfd
void ez(struct Body* b){ if (b->speed == 0)    tick(); }   // int-literal promoted
float gThreshold;
void g(void)            { if (gThreshold > 0.5f) tick(); }
void gq(void)           { if (gThreshold == 2.0f) tick(); }
