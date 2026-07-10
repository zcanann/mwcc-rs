// The MIXED-INIT interleave (extends canaries 1154-1156): a member-LOAD-init local + a const-init
// local under one narrow guard reassigning only the second. Measured model facts:
// - The LOAD issues in the width-op -> compare latency slot, its destination RECLAIMING the dying
//   condition register r3 (unlike a const `li` init, which leaves it alone — fire 647) — r3 is the
//   in-place add's home.
// - A signed-char pointee's `extsb` is SPLIT from its `lbz`, scheduling after the second local's init:
//     clrlwi r0,t,24; lbz r3,0(p); cmplwi r0,4; li r0,8; extsb r3,r3; bne L; li r0,9;
//     L: add r3,r3,r0; blr
// - An int pointee is the same with `lwz` and no extend. An UNSIGNED char pointee is unmeasured and
//   defers. This is __va_arg's `int g_reg = list->gpr` init shape. (fire 649)
int nlf_char(unsigned char t, signed char* p) { int g = *p; int m = 8; if (t == 4) { m = 9; } return g + m; }
int nlf_int(unsigned char t, int* p)          { int g = *p; int m = 8; if (t == 4) { m = 9; } return g + m; }
