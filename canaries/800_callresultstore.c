// `g = h();` storing a call result: the result lands in r3, and mwcc stores from there
// directly (`bl h; stw r3,g; lwz r0,20(r1)`). Ours forced it through the scratch first
// (`mr r0,r3; stw r0`). Removing that move exposed a second issue: the epilogue's
// saved-LR reload then hoisted *past* the store (it no longer touched r0), but mwcc
// keeps a result-store ahead of the reload. The reload hoist now treats a store as a
// barrier (while still overlapping post-call loads / register moves). Float-return
// stores (`gf = hf()`) keep the redundant `fmr` for now (separate, deferred).
int h(void);
int h2(int);
int g1, g2;
void one(void)        { g1 = h(); }            // bl h; stw r3,g1; lwz r0,20(r1)
void witharg(int a)   { g1 = h2(a); }
void two(void)        { g1 = h(); g2 = h(); }  // two calls, two result-stores
