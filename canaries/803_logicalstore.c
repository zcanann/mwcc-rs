// `gi = a && b` / `a || b` as a stored *value*: mwcc builds the 0/1 result in the
// scratch with forward branches to a join, then stores — `cmpwi r3,0; li r0,0; beq X;
// cmpwi r4,0; beq X; li r0,1; X: stw r0`. Ours deferred (the value path errored on the
// logical operator); it now routes to the via-scratch short-circuit emitter, with the
// trailing `mr r0,r0` elided when the result already is the scratch. The return form
// (early `beqlr`) is unchanged.
int g1, g2, g3;
void andv(int a, int b)  { g1 = a && b; }       // forward-branch join, stw r0
void orv(int a, int b)   { g2 = a || b; }
void cmpand(int a, int b){ g3 = (a < b) && a; } // a comparison as the left operand
int andret(int a, int b) { return a && b; }     // beqlr early returns — unchanged
