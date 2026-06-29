// Passing an argument to a NARROW (char/short) parameter applies the C conversion to that
// type. A narrow argument that already fits (a char arg to a char parameter, a char load, an
// in-range constant) needs no conversion and is passed in place. But a WIDER argument — an
// int variable/global, or a computed expression — must be narrowed: `void g(char); g(int_a)`
// is `extsb r3,r3; bl g`. ours did not model that narrowing (it passed the wide value raw, a
// MISCOMPILE: `g(256)` to a `char` parameter must pass 0, not 256), and mwcc schedules the
// extsb into the non-leaf prologue (keystone), so the wide case DEFERS for now.
extern void g_char(char x);
void arg_char(char a)    { g_char(a); }    // char->char: passed in place (no extsb)
void arg_const(void)     { g_char(5); }    // in-range constant: li r3,5
void arg_deref(char* p)  { g_char(*p); }   // char load: already narrow

// DEFERRED (argument wider than the narrow parameter — needs the narrowing conversion the
// keystone prologue scheduler would interleave):
//   void f(int a)        { g_char(a); }       // int variable
//   void f(int a, int b) { g_char(a + b); }   // computed
//   void f(void)         { g_char(global_int); }
