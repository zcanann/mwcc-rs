// A void function whose body is only local reassignments has no observable effect: every
// local is dead (assigned but never stored, passed to a call, or returned), so mwcc
// dead-code-eliminates the whole body and emits just `blr`. The value-tracking path
// inlines locals into the RETURN expression, so a void function had nowhere to inline and
// deferred; recognizing the all-dead-assignment shape emits the empty body directly. A
// store or call in the body is observable and stays on its own path.
int sink;
void dead_one(int a)        { int x; x = a; }            // blr
void dead_two(int a)        { int x, y; x = a; y = a; }  // blr
void dead_expr(int a)       { int x; x = a + 1; }        // blr (the computation is dead too)
void dead_chain(int a)      { int x; x = a; x = x + 1; } // blr
void dead_const(void)       { int x; x = 5; }            // blr
