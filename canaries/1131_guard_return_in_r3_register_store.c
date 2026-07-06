// The register-leaf counterpart to canary 1130: when a guard's early-return value already occupies
// the result register (`if(a>0) return a;`) and the store continuation stores a value that is ALSO
// already in a register (`*p=a`, `*p=b`), mwcc stores it DIRECTLY — no r0 materialization — before the
// return: `if(a>0) return a; *p=b; return 0;` -> `cmpwi r3,0; bgtlr; stw r4,0(r5); li r3,0; blr`. This
// is the distinct emission order from the materialized-value case B (which lands the value in r0 AFTER
// the return). When the return value is the guard value itself (`return a`), the `mr r3,r3` coalesces
// away, leaving `bgtlr; stw r3,0(r4); blr`. Covers `*p`, `p[const]`, `p->member` register stores. (fire 609)
int rs_selfstore(int a, int* p)            { if (a > 0) return a; *p = a;   return 0; }  // bgtlr; stw r3,0(r4); li r3,0
int rs_otherreg(int a, int b, int* p)      { if (a > 0) return a; *p = b;   return 0; }  // bgtlr; stw r4,0(r5); li r3,0
int rs_indexed(int a, int* p)              { if (a > 0) return a; p[2] = a; return 0; }  // bgtlr; stw r3,8(r4); li r3,0
int rs_return_guard(int a, int* p)         { if (a > 0) return a; *p = a;   return a; }  // bgtlr; stw r3,0(r4); blr (mr coalesced)
