// A leaf tail select `(cond) ? G : F` (equivalently `if (cond) return G; return F;`,
// which the parser collapses to the same Conditional) where exactly one arm is a non-zero
// constant. mwcc has two forms, by whether the register leaf is an operand of the
// condition:
//
//   * unrelated leaf — `(c) ? 1 : x`: materialize the constant in the RESULT register and
//     conditionally return it, then the leaf — `cmpwi r3,0; li r3,1; bnelr; mr r3,r4`.
//     The `li r3,1` clobbers only the spent condition operand. (Was a pre-existing diff:
//     ours used an r0-staged branch-materialize instead of the bnelr conditional return.)
//
//   * leaf is a condition operand — `(a>b) ? 7 : b`: the destination-first form would
//     clobber `b` before the move reads it, so mwcc stages the constant in r0 and
//     conditionally moves the leaf over it — `cmpw; li r0,7; bgt L; mr r0,r4; L: mr r3,r0`.
//
// The very common `if (error) return -1; return value;` shape is the first form.
int clamp_unrelated(int c, int x)   { if (c) return 1; return x; }   // li r3,1; bnelr; mr r3,r4
int pick_unrelated(int c, int x)    { return c ? x : 1; }            // li r3,1; beqlr-form; mr r3,x
int err_code(int err, int value)    { if (err) return -1; return value; }
int max_like(int a, int b)          { return a > b ? 7 : b; }        // li r0,7; bgt; mr r0,r4; mr r3,r0
int floor_at(int a, int b)          { return a < b ? 5 : a; }        // leaf a is a condition operand
