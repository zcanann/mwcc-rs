// CHAINED narrow-guarded subset-mutation blocks over const-init locals — the __va_arg fall-through
// skeleton at 2-local scale. The condition parameter stays LIVE across the later tests, so the local
// homes shift PAST it (r4, r5 — not the r3/r0 consumer tree of the single-block form), each subsequent
// test RE-NARROWS into the scratch (measured: mwcc does not carry the narrow value across a join), and
// the join adds the homes into the result:
//   clrlwi r0,t,24; li r4,8; cmplwi r0,2; li r5,4; bne L1; li r4,7;
//   L1: clrlwi r0,t,24; cmplwi r0,3; bne L2; li r5,9; L2: add r3,r4,r5; blr
// Arms reassign any SUBSET in declaration order; all comparison operators work; 3+ blocks chain the
// same way (each if bumps @N by 2). A second live parameter defers (home liveness). (fire 648)
int ncb_two(unsigned char t)   { int a = 8; int b = 4; if (t == 2) { a = 7; }        if (t == 3) { b = 9; } return a + b; }
int ncb_mixed(unsigned char t) { int a = 8; int b = 4; if (t == 2) { a = 7; b = 5; } if (t == 3) { b = 9; } return a + b; }
int ncb_three(unsigned char t) { int a = 8; int b = 4; if (t > 1) { a = 7; } if (t < 5) { b = 9; } if (t == 3) { a = 1; } return a + b; }
// SELF-OP arm values fold against the still-known init (fire 650): `int a=8; if(t==2){ a=a-1; }` emits
// `li r4,7` — exactly __va_arg's `maxsize--` -> `li r5,7`. Valid only while no EARLIER block reassigned
// the local (a branch-dependent value defers).
int ncb_fold(unsigned char t)  { int a = 8; int b = 4; if (t == 2) { a = a - 1; }        if (t == 3) { b = 9; } return a + b; }
int ncb_fold2(unsigned char t) { int a = 8; int b = 4; if (t == 2) { a = a + 5; b = b - 2; } if (t == 3) { b = 9; } return a + b; }
// THREE locals (fire 664): homes extend sequentially past the live condition parameter (a->r4, b->r5,
// c->r6); ALL inits past the first land after the compare (li r5; li r6 both follow cmplwi); the join
// reassociates a+(b+c): `add r3,r5,r6; add r3,r4,r3`. Arms reassign any subset across any block count.
int ncb3(unsigned char t)      { int a = 8; int b = 4; int c = 1; if (t == 2) { a = 7; } if (t == 3) { b = 9; c = 2; } return a + b + c; }
int ncb3_mix(unsigned char t)  { int a = 8; int b = 4; int c = 1; if (t > 1) { a = 7; c = 3; } if (t < 5) { b = 9; } if (t == 3) { c = 2; } return a + b + c; }
// FOUR locals (fire 665): homes r4-r7 sequential; the join reassociates a+((b+c)+d) with the innermost
// pair through the SCRATCH first: `add r0,r5,r6; add r3,r0,r7; add r3,r4,r3` (measured).
int ncb4(unsigned char t) { int a = 8; int b = 4; int c = 1; int d = 6; if (t == 2) { a = 7; d = 5; } if (t == 3) { b = 9; c = 2; } return a + b + c + d; }
// FIVE locals (fire 666): homes r4-r8; the N>=4 join generalizes — the inner sum builds in the
// SCRATCH (`add r0,h1,h2; add r0,r0,h3; …`), the last term lands in the result, h0 tops it.
// Also fire 666: an UNMUTATED const-init local DEFERS (mwcc folds it into the join and compacts the
// homes — measured; the handler previously DIFFed on that shape).
int ncb5(unsigned char t) { int a = 8; int b = 4; int c = 1; int d = 6; int e = 3; if (t == 2) { a = 7; c = 5; e = 2; } if (t == 3) { b = 9; d = 4; } return a + b + c + d + e; }
