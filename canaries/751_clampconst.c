// A select with exactly one non-zero constant arm and the other a register leaf —
// e.g. clamping `x` to a constant bound, `x > 5 ? x : 5`. mwcc stages the constant
// in r0, conditionally moves the leaf over it (a forward branch skips the move when
// the condition picks the constant), then `mr dest, r0` — so the leaf operand (often
// the comparison's own operand, which shares the result register) is not clobbered.
int clamp_lo(int x)            { return x > 5 ? x : 5; }
int clamp_hi(int x)            { return x < 5 ? x : 5; }
int pick_true_leaf(int a, int b)  { return a > b ? a : 5; }
int pick_false_leaf(int a, int b) { return a > b ? 5 : a; }
int floor_at(int x)            { return x < 0 ? x : 100; }
