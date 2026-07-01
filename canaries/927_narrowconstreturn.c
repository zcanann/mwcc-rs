// A CONSTANT narrow (char/short) return is truncated to the return type at COMPILE time and loaded
// directly: mwcc emits `li r3, (type)const`, NOT a runtime `li r0,const; extsb r3,r0`. The previous
// over-extension was a whole-object DIFF (untriggered in the real sweeps, caught by a probe). A value
// outside the narrow range truncates (`(char)300` -> 44). A variable or deref narrow return is
// unaffected: a variable already this narrow stays in place, and a load yields the natural width.
char           c5    (void) { return 5;   }  // li r3,5
char           cneg  (void) { return -1;  }  // li r3,-1
char           ctrunc(void) { return 300; }  // li r3,44   ((char)300)
short          s100  (void) { return 100; }  // li r3,100
short          sneg  (void) { return -50; }  // li r3,-50
unsigned char  uc200 (void) { return 200; }  // li r3,200
unsigned short us1k  (void) { return 1000;}  // li r3,1000
