// `if (cond) tgt = c1; else tgt = c2;` — a single constant store to the same target
// in both arms. mwcc's single-store select idiom (branchless `srawi; addi` for
// consecutive constants, speculate-and-branch into one register otherwise, then ONE
// store) applies ONLY to a direct GLOBAL (SDA-addressed) target. For a POINTER-
// dereference target (`*p`, `p[i]`) mwcc keeps the full two-exit branch form
// (`cmpwi; beq; li; stw; blr; li; stw; blr`).
//
// Regression guard: emit_trailing_if used to route ANY same-target constant if/else
// store through the select path, branchless-ifying pointer stores that mwcc branches —
// a wrong-bytes DIFF. The select path is now gated to global targets; pointer targets
// fall through to the byte-exact two-exit form.

// Pointer-dereference targets — the two-exit branch form.
void ptr_store_consec(int a, int* p)    { if (a) *p = 1; else *p = 2; }
void ptr_store_nonconsec(int a, int* p) { if (a) *p = 7; else *p = 9; }
void ptr_index_store(int a, int* p)     { if (a) p[0] = 1; else p[0] = 2; }

// Direct global targets — the single-store select idiom (branchless / speculate).
int g_consec, g_nonconsec;
void glob_store_consec(int a)    { if (a) g_consec = 1; else g_consec = 2; }
void glob_store_nonconsec(int a) { if (a) g_nonconsec = 5; else g_nonconsec = 9; }
