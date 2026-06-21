// Signed `x <= 0` as a boolean: mwcc's branchless idiom is `cntlzw` (count leading
// zeros) — which is 0 for x<0 and 32 for x==0, but 1..31 for x>0 — then rotate a 1
// left by that count (`rlwnm`) and keep the low bit, which is set only for x <= 0.
// `li d,1` is scheduled after the cntlzw when the operand already sits in d (a leaf
// the cntlzw must read first), otherwise before it.
int leq_param(int x)            { return x <= 0; }
int leq_load(int *p)            { return *p <= 0; }
struct LeqS { int a; };
int leq_member(struct LeqS *s)  { return s->a <= 0; }
int leq_expr(int a, int b)      { return (a - b) <= 0; }
int leq_branch(int x)           { if (x <= 0) return 7; return 9; }
