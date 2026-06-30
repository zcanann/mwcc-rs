// Long long PARAMETERS (the second increment). Each long-long arg occupies an odd-start GPR pair
// — r3:r4, r5:r6, … (high in the lower-numbered register). With an all-long-long parameter list the
// pairs need no alignment skips. Modeled shapes:
//   (int)/(unsigned) truncation -> the LOW word: `mr r3,r4`
//   return a long-long param     -> a bare `blr` (first param) or a pair move (`mr r4,r6; mr r3,r5`)
//   a + b  -> `addc r4,r4,r6 ; adde r3,r3,r5`     (LOW word carries into HIGH)
//   a - b  -> `subfc r4,r6,r4 ; subfe r3,r5,r3`
// A mixed int/long-long list (register-pair alignment), >4 long-long args, and multiply (a runtime
// helper) all defer.
int                truncll(long long a)                                { return (int)a; }
unsigned           utruncll(long long a)                               { return (unsigned)a; }
long long          retll(long long a)                                  { return a; }
long long          retsecond(long long a, long long b)                 { return b; }
long long          addll(long long a, long long b)                     { return a + b; }
long long          subll(long long a, long long b)                     { return a - b; }
unsigned long long addull(unsigned long long a, unsigned long long b)  { return a + b; }
