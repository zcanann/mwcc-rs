// A global-to-global copy `g = h` (both file-scope globals) loads the source into the scratch and
// stores it: `lwz r0,h; stw r0,g` (each address a relocation). Previously the store-value path only
// knew register-resident params/locals and deferred a bare global value ("unknown variable 'h'").
// Word and halfword globals (int / unsigned / short / unsigned short) are byte-exact.
//
// DEFERS (no wrong bytes): a BYTE global (char / unsigned char) — mwcc drops the sign-extension for a
// char->char store (`lbz r0,h; stb r0,g`), which the general `lbz; extsb` load does not model — and a
// pointer global copy.
int            gi,  hi;   void copy_int  (void) { gi  = hi;  }  // lwz r0,hi; stw r0,gi
unsigned       gu,  hu;   void copy_uint (void) { gu  = hu;  }  // lwz; stw
short          gs,  hs;   void copy_short(void) { gs  = hs;  }  // lha r0,hs; sth r0,gs
unsigned short gus, hus;  void copy_ushrt(void) { gus = hus; }  // lhz r0,hus; sth r0,gus
