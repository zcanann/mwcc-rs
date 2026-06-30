// A leaf `void` function whose body is a single non-counting `while (truthy) { pointer++ }` is now
// byte-exact: mwcc does NOT unroll these, it emits the rotated form (`b COND; BODY: addi; COND:
// <test>; bne BODY; blr`) with no frame, the loop-carried pointer increment emitted IN PLACE (not
// value-tracked across the back-edge — the linear value tracker has no back-edge).
//
// Only a POINTER increment under a TRUTHY condition matches. mwcc treats neighbouring shapes very
// differently, and each of those DEFERS rather than emit wrong bytes:
//   - integer increment   `while (x) x++;`      -> counted CTR loop (neg r0,r3; mtctr; bdnz)
//   - counter-vs-bound cmp `while (p < e) p++;` -> counted CTR loop (trip count (e-p)/stride)
//   - do-while            `do p++; while (*p);` -> fuses increment+load into `lwzu r0,4(r3)`
//   - store body          `while (*p) *p = 0;`  -> hoists the loop-invariant store value (LICM)
//   - empty body          `while (*p) {}`       -> no skip branch (condition is the loop top)
void scan_fwd(int *p)   { while (*p) p++; }     // b 8; addi r3,r3,4; lwz r0,0(r3); cmpwi r0,0; bne 4; blr
void scan_back(int *p)  { while (*p) p--; }     // addi r3,r3,-4
void scan_two(int *p)   { while (*p) p += 2; }  // addi r3,r3,8
void scan_truthy(int *p) { while (p) p++; }     // truthy pointer condition (no deref): cmplwi r3,0
// A DATA-DEPENDENT comparison (one side a deref, the other a constant) also rotates: its trip count
// is not computable, unlike the counter-vs-bound `p < e` above which mwcc countifies.
void scan_until(int *p)    { while (*p != 0) p++; } // == the truthy form: lwz; cmpwi r0,0; bne 4
void scan_positive(int *p) { while (*p > 0) p++; }  // lwz; cmpwi r0,0; bgt 4
