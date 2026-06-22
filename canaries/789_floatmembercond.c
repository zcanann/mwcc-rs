// An ordered float comparison whose value is a struct member or file-scope global
// (not a register leaf) loads that value with `lfs`/`lfd` into f1 while the pool
// constant is in f0, then `fcmpo f1,f0` — `if (p->speed > 0)` and `if (gTime > 0.5f)`.
// The integer literal still promotes to the comparison's precision. (`==`/`!=` on a
// member uses a swapped register assignment not yet modeled, and a member compared
// with a float in f1 needs the FP allocator — both deferred, not miscompiled.)
struct Body { int flags; float speed; double mass; };
void tick(void);
void a(struct Body* b) { if (b->speed > 0.0f) tick(); }
void c(struct Body* b) { if (b->speed > 0)    tick(); }
void d(struct Body* b) { if (b->speed <= 1.0f) tick(); }
void e(struct Body* b) { if (b->mass > 0.0)   tick(); }
float gThreshold;
void g(void)            { if (gThreshold > 0.5f) tick(); }
