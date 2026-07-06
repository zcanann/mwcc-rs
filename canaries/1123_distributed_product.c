// Two products sharing a common factor distribute to one add/subf + one mullw: `a*b + a*c` = a*(b+c).
// The factor keeps its side from the FIRST product (`a*b`->`mullw d,a,r0`; `b*a`->`mullw d,r0,a`), the
// sum is source order. A single shared operand (`a*a + a*b` -> a*(a+b)) folds too; the commuted case
// `a*b + b*a` (mwcc factors the other operand) and constant multipliers (`a*2+a*3`) are left alone.
// (fire 598 — found by re-sweeping with a corrected byte-exact probe.)
int distr_left(int a, int b, int c)   { return a*b + a*c; }   // add r0,b,c;  mullw r3,r3,r0
int distr_right(int a, int b, int c)  { return b*a + c*a; }   // add r0,b,c;  mullw r3,r0,r3
int distr_mixed(int a, int b, int c)  { return a*b + c*a; }   // factor left (from first product)
int distr_mixed2(int a, int b, int c) { return b*a + a*c; }   // factor right (from first product)
int distr_shared(int a, int b)        { return a*a + a*b; }   // one shared operand: a*(a+b)
unsigned distr_uns(unsigned a, unsigned b, unsigned c) { return a*b + a*c; }
