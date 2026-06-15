// A non-leaf function doing arithmetic on a call result: mwcc reloads the saved
// LR (lwz r0,20(r1)) immediately after the call, before the post-call work
// (here addi r3,r3,1). A late pass hoists that reload up to right after the bl.
int g(int);
int postinc(int a){ return g(a) + 1; }
