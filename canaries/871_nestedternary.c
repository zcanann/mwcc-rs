// A nested ternary / select chain `cond ? <leaf/const> : (<nested select>)`: mwcc tests the outer
// condition, returns the true arm early when it holds, and emits the nested false arm as the
// fall-through. `a ? b : (c ? b : 0)` is `cmpwi a,0; beq else; mr r3,r4; blr; else: <c?b:0>`. The
// true arm must be a placeable leaf/constant; the false arm is emitted by recursion, so the whole
// thing is byte-exact when the inner select is itself supported and defers cleanly otherwise.
int nest_leaf(int a, int b, int c)         { return a ? b : (c ? b : 0); }
int nest_leaf2(int a, int b, int c, int d) { return a ? b : (c ? b : d); }
int nest_cmp(int a, int b)                 { return a == 0 ? 5 : (a == 1 ? 6 : b); }
