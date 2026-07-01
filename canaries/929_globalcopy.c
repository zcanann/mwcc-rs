// A global-to-global copy `g = h` (both file-scope globals) loads the source into the scratch and
// stores it (each address a relocation). Previously the store-value path only knew register-resident
// params/locals and deferred a bare global value ("unknown variable 'h'"). All integer widths are now
// byte-exact: a NARROW store target truncates, so a signed-narrow global source is read RAW under the
// truncation context (`char gc,hc; gc = hc;` -> `lbz r0,hc; stb r0,gc`, no redundant `extsb` — mwcc
// drops it), while a word/halfword source uses its natural load (`lwz`/`lha`/`lhz`).
//
// DEFERS (no wrong bytes): a pointer global copy.
int            gi,  hi;   void copy_int  (void) { gi  = hi;  }  // lwz r0,hi; stw r0,gi
unsigned       gu,  hu;   void copy_uint (void) { gu  = hu;  }  // lwz; stw
short          gs,  hs;   void copy_short(void) { gs  = hs;  }  // lha r0,hs; sth r0,gs
unsigned short gus, hus;  void copy_ushrt(void) { gus = hus; }  // lhz r0,hus; sth r0,gus
char           gc,  hc;   void copy_char (void) { gc  = hc;  }  // lbz r0,hc; stb r0,gc  (no extsb)
unsigned char  guc, huc;  void copy_uchar(void) { guc = huc; }  // lbz; stb
