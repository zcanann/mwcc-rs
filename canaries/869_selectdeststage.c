// The constant / both-computed branch selects stage the value in r0 only when the computed arm
// reads the DESTINATION register; when it does NOT, mwcc stages directly in the destination and
// conditionally returns — `li r3,-1; bltlr; addi r3,r4,1` (const+computed), or
// `addi r3,r4,-1; bgelr; addi r3,r4,1` (both-computed) — with no r0 staging or trailing `mr`.
// The discriminator is whether the arm reads the result register (`a + 1` with a in r3 does, so
// it stays r0-staged; `b + 1` does not, so it stages in r3). This had been a latent diff: every
// earlier select canary happened to read the condition operand.
int const_nondest(int a, int b) { if (a < 0) return -1; return b + 1; }  // li r3,-1; bltlr; addi r3,r4,1
int both_nondest(int a, int b)  { return (a < 0) ? b + 1 : b - 1; }      // addi r3,r4,-1; bgelr; addi r3,r4,1
int const_dest(int a)           { if (a < 0) return -1; return a + 1; }  // arm reads a=dest -> r0-staged + mr
