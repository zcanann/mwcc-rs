// A single call-result local consumed once in the trailing return is folded into the return
// (`int y = g(); return y + 1;` -> `return g() + 1;`) — byte-exact when the result is combined with a
// constant or returned as-is. But NOT when the return also reads a PARAMETER: then the call result is
// combined with a value live ACROSS the call, and mwcc keeps the result in its register while the
// parameter sits in a callee-saved register (`int y=f(x); return y+x` -> `add r3,r3,r31`), which is
// DIFFERENT bytes from the inlined call-expression form (`return f(x)+x` -> `add r3,r31,r3`). Folding
// that away shipped wrong bytes; it now leaves the local for the callee-saved dispatch (or defers).
//
// This canary pins the SAFE inlines (no parameter in the return). The `return y OP x` param-combine
// cases are deferred (no object to compare), so they are not included here.
int  g(void);
int  h(int);
int  result_plus_const(void) { int y = g(); return y + 1; }   // return g() + 1
int  result_returned(int x)  { int y = h(x); return y; }      // return h(x)  (no param combine)
int  result_shifted(void)    { int y = g(); return y << 2; }  // return g() << 2
int  result_masked(void)     { int y = g(); return y & 255; } // return g() & 255
