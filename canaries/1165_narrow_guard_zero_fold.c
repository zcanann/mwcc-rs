// Narrow-guard tests against ZERO fold the width test into the RECORD form and drop the compare:
// `clrlwi r0,t,24; ...; cmplwi r0,0` becomes `clrlwi. r0,t,24; ...` with cr0 set by the mask
// itself — slot-for-slot identical otherwise (the staged li constants keep their positions, the
// branch options are unchanged). Measured uniformly across every narrow-guard family (fire 674);
// before the fold these six shapes were live DIFFs the canary corpus never covered (every prior
// measurement used a nonzero compare constant).
int zf_assign(unsigned char t) { int y = 3; if (t == 0) { y = 7; } return y; }
int zf_assign_ne(unsigned char t) { int y = 3; if (t != 0) { y = 7; } return y; }
int zf_short(unsigned short t) { int y = 3; if (t == 0) { y = 7; } return y; }
int zf_il2(unsigned char t) { int a = 1; int b = 2; if (t == 0) { a = 5; b = 6; } return a + b; }
int zf_il2_ne(unsigned char t) { int a = 1; int b = 2; if (t != 0) { a = 5; b = 6; } return a + b; }
int zf_il3(unsigned char t) { int a = 1; int b = 2; int c = 3; if (t == 0) { a = 5; b = 6; c = 7; } return a + b + c; }
int zf_chain(unsigned char t) { int a = 1; int b = 2; if (t == 0) { a = 5; } if (t == 1) { b = 6; } return a + b; }
int zf_bittest(unsigned char t, int g) { int a = 1; int b = 2; if (t == 0) { a = 5; if (g & 1) { b = 6; } } return a + b; }
int zf_armload(unsigned char t, int* p) { int a = 1; int b = 2; if (t == 0) { a = *p; b = 6; } return a + b; }
int zf_armload_sc(unsigned char t, signed char* p) { int a = 1; int b = 2; if (t == 0) { a = *p; b = 6; } return a + b; }
// Nonzero constants keep the unfused clrlwi + cmplwi pair (regression anchors).
int zf_keep(unsigned char t) { int y = 3; if (t == 5) { y = 7; } return y; }
int zf_keep_load(unsigned char t, int* p) { int a = 1; int b = 2; if (t == 3) { a = *p; b = 6; } return a + b; }
