// The comma operator `(a, b)`: evaluate the left for its side effects, discard its
// value, yield the right. This was the #1 real-file parser blocker (46 files defer on
// "expected ParenClose, found Comma"). The parser now folds a parenthesized comma chain
// into Expression::Comma; codegen supports it byte-exactly in the two value positions
// where mwcc keeps the right in its natural register:
//
//   * a store value     — `gi = (a, b)` is `stw b,gi` (no move): the left side-effects
//                          register before the target (`gi=(gh=a,b)` => symbols gh,gi).
//   * a flat-arithmetic  — `gi = (a, b) + 1` is `addi r0,b,1` (b used in place), only when
//     binary operand       both peeled operands are leaves/constants and the op is not a
//                          comparison/logical (those route to shapes with pre-existing gaps).
//
// Everything else DEFERS honestly rather than emit a diff: a call in the discarded left
// (the surviving right is live across it — needs the callee-saved allocator), an indexed
// or computed store in the left (mwcc reorders it against the target store), a comparison/
// computed/unary operand, and the return/call-arg/index/condition sub-operand positions.
int gi, gh, gk;
void leaf(int a, int b)            { gi = (a, b); }          // stw b,gi
void chain(int a, int b, int c)    { gi = (a, b, c); }       // stw c,gi
void assign_left(int a, int b)     { gi = (gh = a, b); }     // stw a,gh; stw b,gi (gh before gi)
void two_assigns(int a,int c,int b){ gi = (gh=a, gk=c, b); } // stw a,gh; stw c,gk; stw b,gi
void feed_add(int a, int b)        { gi = (a, b) + 1; }      // addi r0,b,1; stw r0,gi
void feed_mul(int a, int b)        { gi = (a, b) * 3; }      // mulli r0,b,3; stw r0,gi
void both_commas(int a,int b,int c){ gi = (a, b) + (a, c); } // add r0,b,c; stw r0,gi
