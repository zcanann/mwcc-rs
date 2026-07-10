// The CONDITIONAL-DEREF TAIL — __va_arg's ending shape: a pointer parameter reassigned from a
// dereference of ITSELF under a `t == 0` narrow guard, then returned. mwcc uses the RECORD-form width
// test (clrlwi. sets cr0 — `t == 0` on an unsigned narrow folds to the record truthiness test, no
// cmplwi) and loads IN PLACE through the parameter's own register:
//   clrlwi. r0,t,24; bne SKIP; lwz r4,0(r4); SKIP: mr r3,r4; blr
// (fire 668 — the __va_arg consumer-construct family begins)
char* cdt_char(unsigned char t, char* addr) { if (t == 0) { addr = *((char**)addr); } return addr; }
int*  cdt_u16(unsigned short t, int* p)     { if (t == 0) { p = *((int**)p); } return p; }
