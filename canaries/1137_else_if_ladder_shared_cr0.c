// The void leaf else-if LADDER whose consecutive conditions compare the SAME signed-int operand
// against the SAME constant: mwcc emits ONE `cmpwi` and both branches ride that cr0 (`ble` then
// `bge` off the single compare), each arm a store then its own `blr` (the two-exit shape). This is
// the `emit_trailing_if` shared-cr0 case — formerly deferred ("consecutive else-if comparisons that
// reuse the condition register") — now reproduced by reusing cr0 in the else-if recursion. Only a
// SIGNED comparison qualifies (an unsigned operand-vs-zero test folds to bne/beq, which the raw
// reuse would not match, so it stays deferred). Reaching the child branch always arrives via the
// parent's taken forward branch, which leaves cr0 intact — so the reuse is exact.
// (fire 624 — general #21)
void ladder_zero(int a, int* p)  { if (a > 0) { *p = 1; } else if (a < 0) { *p = 2; } else { *p = 3; } }  // cmpwi r3,0; ble; bge
void ladder_five(int a, int* p)  { if (a > 5) { *p = 1; } else if (a < 5) { *p = 2; } else { *p = 3; } }  // cmpwi r3,5; ble; bge
void ladder_noelse(int a, int* p){ if (a > 0) { *p = 1; } else if (a < 0) { *p = 2; } }                    // ...; bge L; ...; bgelr
void ladder_ge_le(int a, int* p) { if (a >= 3) { *p = 1; } else if (a <= 1) { *p = 2; } else { *p = 3; } } // cmpwi r3,3 shared? NO (3!=1) — re-tests
void ladder_mixed(int a, int b, int* p) { if (a > 0) { *p = 1; } else if (a < 0) { *p = 2; } else if (b > 0) { *p = 3; } else { *p = 4; } }  // a-cr0 reused, then b re-tested
