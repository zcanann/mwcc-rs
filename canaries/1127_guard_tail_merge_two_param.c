// A NON-self-referential TWO-parameter reassignment tail after a guard whose value is in the result
// register uses the same r0 tail-merge as the single-parameter case (canary 1125), reading the tail
// from two registers: `int d; if(a>0) return a; d=b+c; return d;` -> `cmpwi; add r0,b,c; ble skip;
// mr r0,a; skip: mr r3,r0; blr`. A register-register `mullw` is scheduled BEFORE the compare to
// overlap its latency; `add`/`subf` (and a constant multiply) stay after. A SELF-referential tail
// (`c=b+c`, the reassigned name still read) keeps its branch form (canary 1126). (fire 603)
int tm2_add(int a, int b, int c)          { int d; if (a > 0) return a; d = b + c; return d; }  // add r0,b,c after cmpwi
int tm2_sub(int a, int b, int c)          { int d; if (a < 0) return a; d = b - c; return d; }  // subf r0
int tm2_mul(int a, int b, int c)          { int d; if (a)     return a; d = b * c; return d; }  // mullw r0 BEFORE cmpwi
int tm2_param(int a, int b, int c, int e) { if (a > 0) return a; c = b + e; return c; }          // reassign a param, non-self-ref
