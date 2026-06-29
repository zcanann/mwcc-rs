// A signed narrow (char/short) local whose value is ALREADY sign-extended by its initializer
// returns byte-exact: a global narrow load appends `extsb` / uses `lha`, an `lha` halfword
// deref sign-extends, and an unsigned narrow zero-extends (so no extension is needed). These
// stay byte-exact; the cases where the value is NOT already extended (a char* `lbz` deref or
// a parameter) DEFER, because inlining the local at its narrow type drops the sign-extension
// the wider return needs (that was a miscompile: `char c = *s; return c;` returned the
// zero-extended byte where mwcc returns the sign-extended char via `lbz; extsb`).
char gc;
int char_from_global(void)        { char c = gc; return c; }          // lbz; extsb (global extends)
int short_from_deref(short* p)    { short c = *p; return c; }         // lha (sign-extends)
int uchar_from_deref(char* s)     { unsigned char c = *s; return c; } // lbz (zero-extends)
int char_deref_computed(char* s)  { char c = *s; return c + 1; }      // c + 1 -> extsb already applied
int int_alias(int a)              { int c = a; return c; }            // same-width alias, byte-exact

// DEFERRED (value not already sign-extended, widening coercion not modeled here):
//   int f(char* s) { char c = *s; return c; }   // char* lbz deref
//   int f(int a)   { char c = a;  return c; }    // parameter (narrowing alias)
//   int f(int a)   { short c = a; return c; }
