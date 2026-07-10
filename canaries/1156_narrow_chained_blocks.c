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
