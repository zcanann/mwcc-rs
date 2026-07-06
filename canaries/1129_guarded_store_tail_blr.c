// A value-returning leaf whose guarded tail is `if (cond) <store>; return <param>;` collapses the
// guard to a branch-conditional-to-link-register (`b<cc>lr`), NOT a forward branch to the final blr:
// `if(a>0) *p=a; return a;` -> `cmpwi r3,0; blelr; stw r3,0(r4); blr`. mwcc always emits the to-link
// form when a conditional branch's destination is the terminal return; a finalization peephole
// (collapse_forward_branch_to_terminal_blr) rewrites the forward branch after label resolution. The
// void forms already did this via emit_trailing_if; this covers the value-return path uniformly, and
// leaves a forward branch intact when code follows the if (target is not the terminal blr). (fire 607)
int      gs_gt(int a, int* p)          { if (a > 0) *p = a; return a; }   // cmpwi; blelr; stw; blr
int      gs_lt(int a, int* p)          { if (a < 0) *p = a; return a; }   // cmpwi; bgelr; stw; blr
unsigned gs_bool(int a, unsigned* p)   { if (a)     *p = 5; return a; }   // cmpwi; beqlr; li r0,5; stw r0; blr
