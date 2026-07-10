// A narrow-guarded arm containing an INNER `&1` record-test one-liner — __va_arg's
// `if (type==2) { size=8; if (g_reg & 1) { even=1; } increment=2; }` core at 2-local scale:
//   clrlwi r0,t,24; li r3,8; cmplwi r0,2; li r5,4; bne JOIN; clrlwi. r0,g,31; li r3,5;
//   beq JOIN; li r5,1; JOIN: add r3,r3,r5; blr
// Measured facts: the inner test is the RECORD-form clrlwi. (keep bit 31, set cr0); the arm's other
// const assign fills its latency slot; BOTH the outer arm-skip and the inner one-liner-skip land on
// the SAME join; b's home AVOIDS the scratch (claimed by the record test) and the live inner operand
// (r4), taking r5 — while a reclaims the dying outer-condition register r3. (fire 651)
int ngib(unsigned char t, int g) { int a = 8; int b = 4; if (t == 2) { a = 5; if (g & 1) { b = 1; } } return a + b; }
