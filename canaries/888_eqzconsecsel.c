// `(x == 0) ? c1 : c2` with consecutive constants — the cntlzw 0/1-flag idiom (a separate detector
// from the sign-bit forms, since == can't use a sign mask). cntlzw is 32 iff x==0; the flag
// (cntlzw>>5)&1 is (x==0)?1:0. Lower-true arm: rlwinm r0,r0,27,31,31 (flag in scratch); neg d,r0;
// addi d,c2 -> c2-(x==0). Higher-true arm: srwi d,r0,5; addi d,c2 -> c2+(x==0). Signedness-agnostic.
// Only the in-register destination is emitted; the scratch (store) destination uses a different
// mwcc register layout and is deferred (not shipped as DIFF). Also unblocks guard-chain ==0 tails
// (added to select_folds_branchless). Completes the <0/>0/>=0/!=0/<=0/==0 consecutive-select family.
int eqz_lo(int a)  { return a == 0 ? 2 : 3; }
int eqz_hi(int a)  { return a == 0 ? 3 : 2; }
int eqz_grd(int a) { if (a < 0) return 1; if (a == 0) return 2; return 3; }
