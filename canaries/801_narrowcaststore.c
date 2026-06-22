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
// A non-leaf integer operand evaluates into the scratch and the store truncates — no
// `extsh`/`extsb` either (`add r0,r3,r4; sth r0`).
void si2(int a, int b) { gs = (short)(a + b); }  // add r0,r3,r4; sth r0
void ci2(int a, int b) { gc = (char)(a & b); }   // and r0,r3,r4; stb r0
void sh(int a)         { gs = (short)(a << 2); } // slwi r0,r3,2; sth r0
