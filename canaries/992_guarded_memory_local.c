// The raise() core shape: a memory-loaded local READ BY a guard, then live across
// calls in r31. The scalar form stages through r0 -- `lwz r0,gi; cmpwi r0,0;
// mr r31,r0; bne CONT; li r3,-1; b EPILOGUE; CONT: bl; mr r3,r31; EPILOGUE:` -- with
// the `mr r31,r0` riding the compare latency; the array form loads r31 directly and
// compares it. The early return branches to the shared epilogue.
int gi;
int arr[6];
extern void g(void);
extern void h(void);

int scalar_guard(void)      { int t = gi; if (!t) return -1; g(); return t; }
int array_guard(int i)      { int t = arr[i]; if (!t) return 0; g(); return t; }
int guard_two_calls(void)   { int t = gi; if (!t) return -1; g(); h(); return t; }
int compare_guard(void)     { int t = gi; if (t == 5) return 9; g(); return t; }
