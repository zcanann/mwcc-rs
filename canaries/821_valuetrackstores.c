// A void function whose value-tracked locals exist only to feed memory stores: `int x =
// a; gi = x; x = b; gj = x;`. The value-tracking path inlines locals into a RETURN
// expression, so a void function (no return) had nowhere to fold them and deferred.
// inline_store_only_locals tracks each local's value sequentially and substitutes it into
// every store, eliminating the locals — `gi = a; gj = b;` — then recompiles, so the store
// fills (or the un-schedulable-store deferral) own the cleaned body. Calls in the body or
// in a local's value are side effects and still defer (the keystone allocator's job).
int gi, gj;
void store_local(int a)              { int x; x = a; gi = x; }                 // gi = a
void reassign_between(int a, int b)  { int x; x = a; gi = x; x = b; gj = x; }  // gi = a; gj = b
void initialized(int a)              { int x = a + 1; gi = x; }               // gi = a+1
void computed_and_const(int a)       { int x; x = a + 1; gi = x; gj = 5; }    // computed+const fill
void two_locals(int a, int b)        { int x, y; x = a; y = b; gi = x; gj = y; }
void chained(int a)                  { int x; x = a; x = x + 1; gi = x; }     // gi = a+1
