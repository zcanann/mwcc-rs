// A local that purely aliases another variable (`T* q = p;`) and is then
// dereferenced must inline the alias: the straight-line path materialized the alias
// in a register and picked r0, emitting an invalid `lwz rD,off(0)` (r0 in a load's
// base position means literal 0, not the register). Substituting `q -> p` yields
// mwcc's plain `lwz rD,off(rP)`. Scalar aliases (`int y = x;`) already worked.
struct S { int a, b; };
int member(struct S *p) { struct S *q = p; return q->b; }
int deref(int *p)       { int *q = p; return *q; }
int scalar(int x)       { int y = x; return y + 1; }
