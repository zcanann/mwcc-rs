// Signed absolute-value selects across every zero-comparison spelling and both tail/value contexts.
// The 2.4.x line uses a sign-mask ALU idiom; build 163 preserves the conditional negate.

int abs_lt_tail(int x) { return x < 0 ? -x : x; }
int abs_le_tail(int x) { return x <= 0 ? -x : x; }
int abs_gt_tail(int x) { return x > 0 ? x : -x; }
int abs_ge_tail(int x) { return x >= 0 ? x : -x; }

int abs_lt_value(int x) { return (x < 0 ? -x : x) + 3; }
int abs_le_value(int x) { return (x <= 0 ? -x : x) + 3; }
int abs_gt_value(int x) { return (x > 0 ? x : -x) + 3; }
int abs_ge_value(int x) { return (x >= 0 ? x : -x) + 3; }
