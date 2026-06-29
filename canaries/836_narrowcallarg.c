// A narrow (char/short) argument passed to a parameter that is NOT wider keeps the value as
// is — no int promotion. `void g(char); g(char_a)` is just `bl g`, not `extsb r3,r3; bl g`,
// because the char parameter reads only the low byte. (Only a WIDER parameter, e.g.
// `void g(int)`, widens the argument — and that case is still ordering-blocked on the
// prologue scheduler, so it is not asserted here.)
void take_char(char);
void take_short(short);
void take_uchar(unsigned char);
void take_two_char(char, char);
void pass_char(char a)              { take_char(a); }        // bl, no extsb
void pass_short(short a)            { take_short(a); }       // bl, no extsh
void pass_uchar(unsigned char a)    { take_uchar(a); }
void pass_two_char(char a, char b)  { take_two_char(a, b); } // a in r3, b in r4, no extsb
