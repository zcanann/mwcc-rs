// Calling through a *global* function pointer: load it into r12, mtctr, bctrl. mwcc
// hoists a leading `li` argument into the mflr->save latency gap just like a direct
// call — `stwu; mflr r0; li r3,5; stw r0,20(r1); lwz r12,gfp; mtctr; bctrl`. The
// saved-LR-store schedule's leading-`li` hoist only fired before a `bl`; an indirect
// `bctrl` (preceded by the fn-ptr `lwz r12`) now qualifies too.
int (*gfp1)(int);
void (*gfp2)(int, int);
int (*gfp0)(void);
int gi;
void one(void)  { gfp1(5); }       // li r3,5 hoisted; lwz r12,gfp1; mtctr; bctrl
void two(void)  { gfp2(3, 7); }    // two arg lis
void ret(void)  { gi = gfp0(); }   // no-arg call, result stored
