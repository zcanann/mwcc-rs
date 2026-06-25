// `if (cond) tgt = c1; else tgt = c2;` with NON-consecutive constants stores a
// branch-materialized select (mwcc branches only for non-consecutive constants;
// consecutive ones take the branchless mask): `cmpwi r3,0; li r0,c2; b<!cond> join;
// li r0,c1; join: stw r0`. The false arm goes first, a forward branch on the false
// condition skips the true arm, then one store. Previously the value/store form had no
// branch-materialize path and deferred on the constant arms; consecutive constants and
// the return form are unchanged.
int gi;
void eq(int a)        { if (a == 0) gi = 10; else gi = 20; } // cmpwi; li 20; bne; li 10
void gt(int a)        { if (a > 0)  gi = 5;  else gi = 9; }  // wide gap, > 0
void truth(int a)     { if (a)      gi = 100; else gi = 7; } // truthiness
void cmp(int a, int b){ if (a == b) gi = 1;  else gi = 8; }  // two-operand ==
