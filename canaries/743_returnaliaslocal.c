// A single-assignment local that just feeds the return — computed before a call,
// returned after it — is preserved through the call exactly as the underlying
// parameter expression would be: `int z = x + 1; g(); return z;` is identical to
// `g(); return x + 1;` (x preserved in r31, the `+1` applied after the call).
extern void ral_step(void);
int ral_alias(int x)  { int z = x;     ral_step(); return z; }
int ral_plus(int x)   { int z = x + 1; ral_step(); return z; }
int ral_scale(int x)  { int z = x * 2; ral_step(); return z; }
