// Consecutive-constant sign selects for `!= 0` and `<= 0` (extending <0/>0/>=0). The mask whose
// sign bit holds the relation is built by a preamble — `neg;or` for !=0, `neg;orc` for <=0 —
// then srawi/srwi + addi, same as the existing forms. `a!=0?2:3` -> neg;or;srawi;addi 3;
// `a!=0?3:2` -> neg;or;srwi;addi 2; `a<=0?2:3` -> neg;orc;srawi;addi 3. The reverse `<=0?3:2`
// and all `==0?` use a different cntlzw idiom and still defer (NOT shipped as DIFF). This also
// unblocks guard-chain tails of these relations (via select_folds_branchless). SignConsecutive
// gained a MaskPreamble enum (None/Andc/Or/Orc) replacing the positive bool.
int ne_lo(int a) { return a != 0 ? 2 : 3; }
int ne_hi(int a) { return a != 0 ? 3 : 2; }
int le_lo(int a) { return a <= 0 ? 2 : 3; }
