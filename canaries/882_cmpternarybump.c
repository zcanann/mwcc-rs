// A `(comparison) ? 1 : 0` TERNARY also advances mwcc's anonymous-@N counter by 3 (like the
// non-comparison `cond ? 1 : 0` of canary 881 and a float branch) — a DIRECT `return a > b` does
// not. control_flow.rs bumps anonymous_label_bump in the comparison-ternary routing too, guarded
// to INTEGER comparisons (a float comparison condition bumps in its own anonymous block). Only
// observable in a FRAME function's extab/extabindex numbering (indirect call forces a frame), and
// the per-function bump composes correctly across a multi-function object's per-object @N counter.
// (`(cmp) ? 0 : 1` correctly matches @N too but has a separate flipped-idiom regalloc diff, so it
// is not exercised here.)
int gt0(int (*g)(int), int a) { return g(a) > 0 ? 1 : 0; }
int lt0(int (*g)(int), int a) { return g(a) < 0 ? 1 : 0; }
