// In a non-leaf function, mwcc fills the mflr->LR-store latency slot with a leading
// register move — a parameter copy ready at entry: `stwu; mflr r0; mr rD,rS; stw
// r0,20(r1)`. Passing a non-first parameter as the sole call argument is exactly
// that hoistable move. Constant and memory loads are NOT scheduled into the slot.
void sink(int);
void move_second(int a, int b)       { sink(b); }        /* mr r3,r4 hoisted */
void move_third(int a, int b, int c) { sink(c); }        /* mr r3,r5 hoisted */
int counter;
void const_first(void)               { sink(5); }        /* li r3,5 — not hoisted */
void global_first(void)              { sink(counter); }  /* load — not hoisted */
