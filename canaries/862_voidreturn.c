// A bare `return;` ends a void function with no value — like reaching the closing brace, it
// produces no code (the function epilogue is the whole tail). It previously failed to parse
// ("expected an expression, found Semicolon"), so any void function written with an explicit
// trailing `return;` deferred. `*p = 5; return;` is byte-identical to `*p = 5;`. (An EARLY void
// return mid-body — `if (c) return; <more statements>` — still needs branch-to-epilogue codegen
// and remains deferred.)
void g(void);
void st_ret(int *p)   { *p = 5; return; }            // store then void return
void call_ret(void)   { g(); return; }               // call then void return
void two_ret(int *p)  { p[0] = 1; p[1] = 2; return; }  // multiple stores then return
void empty_ret(void)  { return; }                    // a lone void return
