// Guard CHAINS (two or more `if (cond) return X;` before the fall-through return). mwcc
// lowers each non-last guard whose value already sits in the result register as a
// conditional return that FALLS THROUGH to the next guard (`cmpwi; bnelr`), not a forward
// branch over the return. The last guard fuses with the fall-through as a branchless select
// (the same form as a lone guard). Params: a=r3, b=r4, c=r5, d=r6.
//
//     if(c) return a; if(d) return b; return 0;
//        -> cmpwi r5,0; bnelr; neg r0,r6; or r0,r0,r6; srawi r0,31; and r3,r4,r0; blr
//
// A non-last guard whose value is NOT in the result register keeps the forward-branch form
// (move the value into the result, return, branch skips it when not taken).
int gi;
int chain_first_in_r3(int a, int b, int c, int d) { if (c) return a; if (d) return b; return 0; }  // bnelr + select
int chain_first_in_r4(int a, int b, int c, int d) { if (c) return b; if (d) return a; return 0; }  // branch-over first
int chain_three_leaf(int a, int b, int c, int d)  { if (c) return a; if (d) return b; return c; }  // 3 leaves

// `if (c) return X; return X` is degenerate (both paths identical). mwcc keeps the dead
// condition test then a single `blr`; we defer rather than emit a spurious conditional
// return — so this shape must NOT appear here (it would not compile byte-exact). Kept as a
// comment to document the deferral:
//   int degenerate(int a, int c) { if (c) return a; return a; }   // DEFERRED
