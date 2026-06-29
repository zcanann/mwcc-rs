// A local of the SAME width as its initializer is a pure copy and value-tracks/inlines
// byte-exact (int from int, char from a same-width char load). A NARROWING narrow local —
// a char/short initialized from a WIDER value (`char c = a;` for an int `a`) — is a
// truncation; inlining it raw drops the `(char)` truncation AND the int sign-extension, which
// was a MISCOMPILE: `char c=a; gi=c;` stored the full int (not (int)(char)a), and `char f(int
// a){ char c=a; return c; }` returned the raw int where mwcc emits `extsb r3,r3`. Those defer
// now (in value_tracking, the single-local return, and inline_store_bearing_locals).
int gi, gj;
int copy_int(int a)                 { int c = a; return c; }          // same width, byte-exact
void store_int_copy(int a)          { int c = a; gi = c; }            // same width
void store_int_twice(int a)         { int x = a; gi = x; gj = x; }    // multi-store, same width
int  deref_char_computed(char* s)   { char c = *s; return c + 1; }    // char-from-char, same width

// DEFERRED (narrowing narrow local, truncation + sign-extension not modeled on inline):
//   void f(int a)  { char c = a;  gi = c; }
//   char f(int a)  { char c = a;  return c; }
//   void f(int a)  { short c = a; gi = c; }
