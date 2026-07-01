// Value-tracking inlines MULTIPLE constant-initialized locals and folds the resulting
// constant arithmetic, matching mwcc: `int k=3; int m=4; return x+k+m` -> `x+3+4` ->
// `addi r3,r3,7` (not `li;li;add;add`). Previously the multi-term-sum guard deferred any
// value-tracked local folded into an additive chain; that only matters for a COMPUTED
// local (mwcc keeps it in a register), so it is relaxed when every tracked value is a
// constant (the inlined form is the direct multi-term-with-constants form we already fold).
int add_two(int x)  { int k = 3; int m = 4; return x + k + m; }   // -> addi 7
int sub_two(int x)  { int k = 3; int m = 4; return x - k - m; }   // -> addi -7
int mix(int x)      { int k = 3; return x + k + 1; }              // -> addi 4
int prod(int x)     { int k = 2; int m = 3; return x + k * m; }   // -> addi 6
