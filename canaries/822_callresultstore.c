// `int x = foo(...); gi = x;` — a single local whose value is exactly one call, used once
// as the WHOLE value of one global store. The call result lives in r3 and is not live
// across any other call, so mwcc stores it directly (`gi = foo(...)` is already byte-exact)
// — no callee-save needed. inline_single_call_result_store folds the local and recompiles.
// This is the trivial entry into value-tracking-with-calls. A second call, a second use of
// the result, the result fused with arithmetic, or a non-void return still need the callee-
// saved register allocator and defer.
int produce(int);
int produce2(int, int);
int side(void);
int gi, gj;
void init_field(int a)      { int x = produce(a); gi = x; }        // gi = produce(a)
void assigned_form(int a)   { int x; x = produce(a); gi = x; }     // same via assignment
void no_args(void)          { int x = side(); gi = x; }            // gi = side()
void two_args(int a, int b) { int x = produce2(a, b); gj = x; }    // gj = produce2(a, b)

// The result may instead be RETURNED — `int x = foo(...); return x;` — same shape, the
// call result already in r3 is the return value (`return foo(...)` is byte-exact). A second
// use (store AND return), a non-void store sink with a return, or the result fused with
// arithmetic still defer.
int ret_result(int a)       { int x = produce(a); return x; }     // return produce(a)
int ret_assigned(int a)     { int x; x = produce(a); return x; }  // same via assignment
int ret_no_args(void)       { int x = side(); return x; }         // return side()

// The single-use call result may be fused with a CONSTANT before the store/return —
// `int x = foo(a); gi = x + 1;` -> `gi = foo(a) + 1;` (byte-exact, the result read in r3).
// The call is substituted into its one use; reading it twice, or fusing it with a value
// live across the call, still defers.
int gk2;
void store_plus_const(int a)   { int x = produce(a); gk2 = x + 1; }   // gk2 = produce(a)+1
int ret_plus_const(int a)      { int x = produce(a); return x & 255; } // return produce(a)&255
int ret_times(int a)           { int x = produce(a); return x * 2; }   // return produce(a)*2
