// The computed-arm branch select also applies in a STORE context — `*p = (cond) ? const : <e>`
// and `*p = (cond) ? <e1> : <e2>` — where the value is staged in r0 and the store writes r0
// directly (no `mr`). So `*p = (a<0) ? -1 : a+100` is
// `cmpwi r4,0; li r0,-1; blt skip; addi r0,r4,100; skip: stw r0,0(r3)`. The handlers now fire
// when the destination is r0 (a store/value-into-r0 context) as well as in tail position; a
// destination that is a real register other than r0 still defers.
void st_const_computed(int *p, int a) { *p = (a < 0) ? -1 : a + 100; }     // stw form, const arm
void st_both_computed(int *p, int a)  { *p = (a < 0) ? a + 1 : a - 1; }    // stw form, both computed
int gv;
void st_global(int a)                 { gv = (a < 0) ? -1 : a + 1; }       // global store select
