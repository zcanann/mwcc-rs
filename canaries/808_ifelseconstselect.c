// `if (cond) tgt = c1; else tgt = c2;` — both arms a single constant store to the same
// target — is one store of a select, not two branches. mwcc branchless-ifies it (a
// truth/sign mask plus an offset) and stores once; ours emitted a forward branch with a
// store in each arm. emit_trailing_if now routes the pattern through the conditional
// store path (byte-exact where the select idiom applies, else an honest defer — strictly
// better than the two-branch diff). A comparison condition with constant arms still
// defers (the "constants in trees" parser gap), and non-constant arms / differing
// targets / call arms keep the branch form.
int gi;
void truth(int a) { if (a)     gi = 1; else gi = 2; }  // (a!=0) select
void sign(int a)  { if (a < 0) gi = 1; else gi = 0; }  // srwi sign bit
