// mwcc collapses the bitwise distributive laws to one inner op plus the outer, sharing
// the common factor: (x&y)|(x&z) -> x&(y|z), (x|y)&(x|z) -> x|(y&z), (x&y)^(x&z) ->
// x&(y^z). The common factor in the first operand position rewrites cleanly; a distinct
// factor or a non-distributive pairing is left as the direct two-and/or form.
int o1, o2, o3, o4;
void and_or(int a, int b, int c)  { o1 = (a & b) | (a & c); }   // -> a & (b | c)
void or_and(int a, int b, int c)  { o2 = (a | b) & (a | c); }   // -> a | (b & c)
void and_xor(int a, int b, int c) { o3 = (a & b) ^ (a & c); }   // -> a & (b ^ c)
void mixed(int a, int b, int c)   { o4 = (a & b) | (c & a); }   // common factor either side
