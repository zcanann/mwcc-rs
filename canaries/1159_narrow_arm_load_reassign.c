// An ARM deref-load reassign in the 2-local interleave — __va_arg's type==3 `g_reg = list->fpr`
// shape: the load emits into the local's home (the in-place r3), the arm's const fills its latency
// slot, and a signed-char pointee's `extsb` SPLITS after it (the fire-649 split, now inside an arm):
//   clrlwi r0,t,24; li r3,1; cmplwi r0,3; li r0,8; bne JOIN; lbz r3,0(p); li r0,9; extsb r3,r3;
//   JOIN: add r3,r3,r0; blr                            (lwz and no extend for an int pointee)
// (fire 652)
int nalr_char(unsigned char t, signed char* p) { int g = 1; int m = 8; if (t == 3) { g = *p; m = 9; } return g + m; }
int nalr_int(unsigned char t, int* p)          { int g = 1; int m = 8; if (t == 3) { g = *p; m = 9; } return g + m; }
