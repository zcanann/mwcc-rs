// A 2-argument call passing the same leaf value twice — `g(a, a)` — places the value in the first
// argument register (already there: a passthrough, no evaluation), then copies it to the second with a
// single `mr r4,r3` that hoists into the non-leaf prologue slot (`mflr r0; mr r4,r3; stw r0,20(r1);
// bl g`). The clobber guard previously deferred this because the repeat "uses" r3 — but an in-place
// passthrough clobbers nothing and stays live for the copy.
//
// DEFERS (no wrong bytes): 3+ arguments (multiple trailing moves need the full argument scheduler),
// and a COMPUTED repeat (`g(a+1, a+1)`, whose arg0 evaluates into r3 that the repeat still needs).
void gv(int, int);
int  gi(int, int);
void gp(int *, int *);
void call_rep_void(int a)  { gv(a, a); }          // mflr; mr r4,r3; stw; bl gv
int  call_rep_int (int a)  { return gi(a, a); }   // mr r4,r3; bl gi  (result stays in r3)
void call_rep_ptr (int *p) { gp(p, p); }          // mr r4,r3; bl gp
