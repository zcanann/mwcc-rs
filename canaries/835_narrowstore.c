// Storing a value to a target of the SAME class/width is byte-exact: int->int, char->char,
// short->short, and an int-pointer deref to an int. A NARROW value (char/short parameter, or
// a narrow memory load) stored to a WIDER integer target needs widening first (mwcc:
// `extsb r0,r3; stw r0,gi`), and which register it widens into depends on whether the value
// is also reused/returned (r3 in place) — an allocator decision not modeled, so those DEFER
// rather than store the raw byte/halfword (which was a miscompile: `gi = a;` for a `char a`
// stored the un-sign-extended byte).
int   gi;
char  gc;
short gs;
void store_int(int a)          { gi = a; }     // stw, no widening
void store_char_to_char(char a){ gc = a; }     // stb, no widening
void store_short_to_short(short a){ gs = a; }  // sth
void store_int_deref(int* p)   { gi = *p; }    // lwz; stw

// DEFERRED (narrow value -> wider int target, widening coercion + register choice not modeled):
//   void f(char a)  { gi = a; }            // extsb r0,r3; stw r0,gi
//   void f(short a) { gi = a; }            // extsh
//   void f(char* s) { gi = *s; }           // lbz; extsb; stw
//   int  f(char a)  { gi = a; return a; }  // extsb r3,r3 in place (value reused)
