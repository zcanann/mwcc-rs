// A binary over TWO COMPOUND-load operands — each wrapping a memory load in an op, e.g.
// `p->x*p->x + p->y*p->y` — DEFERS: mwcc hoists both loads to the top (`lwz;lwz;mullw;mullw;add`)
// with an allocator-chosen register assignment ours does not reproduce; the generic combine
// interleaves them (`lwz;mullw;lwz;mullw;add`) — same result, WRONG bytes. Per "byte-exact or
// defer", we defer (guard: is_compound_load on BOTH sides) rather than ship the mis-schedule.
// These NEIGHBORING shapes must STAY byte-exact (a too-broad guard regressed 846/850 — the oracle
// caught it; this canary pins the boundary):
int param_sq_sum(int a, int b)       { return a*a + b*b; }    // params in regs, no loads
int single_product(int *p)           { return p[0]*p[0]; }    // one compound load: lwz; mullw r,r0,r0
int two_bare_loads(int *p)           { return p[0] + p[1]; }  // bare loads adjacent: lwz; lwz; add
int two_deref_loads(int *p, int *q)  { return *p + *q; }
int load_plus_const(int *p)          { return *p + 5; }
struct P { int x, y; };
int member_product(struct P *p)      { return p->x * p->y; }  // two bare member loads
int deref_offset_sum(int *p)         { return *(p+1) + *(p+2); } // bare loads via *(p+C)
