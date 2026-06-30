// A `cond ? 1 : 0` / `? 0 : 1` TERNARY advances mwcc's anonymous-@N counter by 3 — like a float
// conditional branch — EVEN when it lowers to a branchless `cond != 0` / `cond == 0` comparison.
// A direct `cond != 0` does NOT bump. The +3 is only observable in a FRAME function's
// extab/extabindex @N numbering (a leaf function has no anonymous symbols), so these use an
// indirect call to force a frame. control_flow.rs bumps anonymous_label_bump in the bool-ternary
// routing. (Previously believed @N diverged only across multi-function objects; this is single-fn.)
int pred1(int (*fp)(void))   { return fp() ? 1 : 0; }
int pred0(int (*fp)(void))   { return fp() ? 0 : 1; }
