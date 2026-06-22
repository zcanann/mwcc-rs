// A chained assignment `g = h = a` stores the same source register to every target.
// mwcc reuses that register directly (`stw r3,h; stw r3,g`); ours staged it through
// the scratch first (`mr r0,r3; stw r0; stw r0`). place_store_value now emits the inner
// store and yields the source register when the chain's ultimate value is a leaf. A
// constant or computed value (`g = h = 5`, `g = h = a+b`) already flows through r0.
int g, h, k;
void two(int a)   { g = h = a; }       // stw r3,h; stw r3,g
void three(int a) { g = h = k = a; }   // stw r3,k; stw r3,h; stw r3,g
void konst(void)  { g = h = 5; }       // li r0,5; stw r0,h; stw r0,g — unchanged
void calc(int a, int b) { g = h = a + b; } // add r0,r3,r4; stw r0,h; stw r0,g
