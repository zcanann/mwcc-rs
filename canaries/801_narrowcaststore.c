// A narrowing cast `(short)x`/`(char)x` stored to a same-width location: the `sth`/`stb`
// truncates, so the cast's sign/zero extension is redundant and mwcc omits it. An int
// leaf stores straight from its register (`sth r3`); a float leaf converts with fctiwz
// but does not narrow (`fctiwz; ...; lwz r0; sth r0` — no `extsh`). A WIDER store keeps
// the extension (`gi = (short)a` genuinely sign-extends to int).
short gs;
signed char gc;
unsigned short gus;
int gi;
void si(int a)    { gs = (short)a; }          // sth r3 — no extsh
void sf(float a)  { gs = (short)a; }          // fctiwz; stfd; lwz r0; sth r0
void ci(int a)    { gc = (char)a; }           // stb r3 — no extsb
void cf(float a)  { gc = (char)a; }           // fctiwz; ...; stb r0
void ui(int a)    { gus = (unsigned short)a; }// sth r3
void widen(int a) { gi = (short)a; }          // extsh r3,r3; stw — extension kept
