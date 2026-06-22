// A non-leaf `if (<cond>) <call>;` schedules the condition test into the prologue's
// mflr->LR-store latency slot — but ONLY for a bare register compare. A member or
// dereference condition loads into r0, which would clobber the just-saved link
// register before it is stored (saving the loaded value, not the return address — a
// corrupt return). mwcc emits the LR store first in that case; ours now does too.
struct S { int flag; };
void sink(void);
void on_member(struct S *p) { if (p->flag) sink(); }
void on_deref(int *p)       { if (*p) sink(); }
void on_reg(int c)          { if (c) sink(); }
