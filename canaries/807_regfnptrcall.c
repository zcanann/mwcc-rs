// Calling through a function-pointer *parameter* with arguments: the pointer is copied
// `mr r12,fp`, then `mtctr; bctrl`. mwcc hoists BOTH the `mr r12` and a leading `li`
// argument into the mflr->save latency gap — `mflr r0; mr r12,r3; li r3,9; stw r0,20`.
// Ours hoisted only the `mr` (the leading-arg-moves pass excluded `li` forms, a==0),
// leaving the `li` behind the save; it now rides along when it follows a hoisted move.
void one(void (*fp)(int))         { fp(9); }       // mr r12,r3; li r3,9; stw
void two(void (*fp)(int, int))    { fp(3, 7); }    // mr r12; li r3,3; (li r4,7 after save)
int  ret(int (*fp)(int), int x)   { return fp(x); }// mr r12,r3; mr r3,r4; stw
