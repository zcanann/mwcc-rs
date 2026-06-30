// `(a == K) ? C : 0` (K, C non-zero) forms the equality mask with no compare: `addi t,a,-K;
// subfic r0,a,K; nor d,t,r0` makes a word whose sign bit is set iff a==K (both a-K and K-a are 0
// then, so the NOR is all-ones); `srawi d,d,31; and d,C,d` keeps C only when set. So `(a==2)?20:0`
// is `addi r4,r3,-2; subfic r0,r3,2; nor r3,r4,r0; li r0,20; srawi r3,r3,31; and r3,r0,r3`. This
// is what a ternary chain recurses into, so it completes `a==1?x:a==2?y:0`. (K==0 uses a different
// cntlzw mask and is left to the other handlers; a non-constant true arm or a store destination
// defers.)
int eq2(int a)    { return (a == 2) ? 20 : 0; }
int eq5(int a)    { return (a == 5) ? 100 : 0; }
int eqneg(int a)  { return (a == -3) ? 7 : 0; }
int chain(int a)  { return a == 1 ? 10 : (a == 2 ? 20 : 0); }
int chain3(int a) { return a == 0 ? 1 : (a == 1 ? 2 : (a == 2 ? 3 : 0)); }
