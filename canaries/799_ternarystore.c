// `g = cond ? b : c` with two non-constant leaf arms: the branch-select lands the
// result in the false arm's register (`cmpwi; beq; mr c,b`), and mwcc stores from
// that register directly (`stw c`). Ours forced the value into the scratch first
// (`mr r0,c; stw r0`) — functionally correct but a byte-diff. The store now uses the
// select's own register. The return form (which moves into r3) was already correct.
int g1, g2, g3;
void sel(int a, int b, int c)  { g1 = a ? b : c; }       // cmpwi; beq; mr r5,r4; stw r5
void swp(int a, int b, int c)  { g2 = a ? c : b; }       // false arm is b -> mr r4,r5; stw r4
void cmp(int a, int b, int c)  { g3 = (a < b) ? b : c; } // comparison condition
int ret(int a, int b, int c)   { return a ? b : c; }     // mr r3,r5 — unchanged
