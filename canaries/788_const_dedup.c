// mwcc pools each distinct read-only constant once per object. Several functions
// comparing against the same `0.0f` share a single `.sdata2` slot and a single `@N`
// symbol — a later reuse consumes no new pool entry, symbol, or number (so the
// reusing function's unwind `@N` shift down). A different value gets its own slot,
// and a single `0.0f` and a double `0.0` stay distinct (different `lfs`/`lfd` widths).
void sink(void);
void a(float x)  { if (x > 0.0f) sink(); }   // first use of 0.0f -> its own slot/@N
void b(float x)  { if (x > 0.0f) sink(); }   // reuses a's 0.0f slot and symbol
void c(float x)  { if (x > 1.5f) sink(); }   // distinct 1.5f -> new slot/@N
void d(float x)  { if (x > 0.0f) sink(); }   // reuses a's 0.0f again
void e(double x) { if (x > 0.0)  sink(); }   // double 0.0 -> distinct from the single
