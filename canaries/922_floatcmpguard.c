// A guard whose condition is a FLOAT comparison against a float CONSTANT (`if(a>0.0f) return 1;
// return 0;`) folds to the branchless 0/1, but mwcc allocates the if's folded-away branch labels
// BEFORE the pooled float constant, so the constant's anonymous @N symbol number is offset by 2 ->
// that case DEFERS (the low-value @N seam). These stay byte-exact: the comparison VALUE itself (no
// phantom labels), a two-variable float-compare guard (pools no constant), and an integer guard.
int fcmp_val(float a)             { return a > 0.0f; }               // fcmpo; mfcr; rlwinm (0/1)
int fcmp_guard2(float a, float b) { if (a < b) return 1; return 0; }// two vars, no @N -> byte-exact
int icmp_guard(int a)             { if (a > 0) return 1; return 0; }// integer guard -> byte-exact
