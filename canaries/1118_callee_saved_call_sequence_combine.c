// Two parameters, each passed to its own call, both live to a combining return:
// `g(a); h(b); return a OP b;`. mwcc keeps BOTH in callee-saved registers — the last parameter in
// r31, the first in r30 — saving them interleaved up front (`stw r31; mr r31,b; stw r30; mr r30,a`).
// The FIRST call reads its parameter from the still-live incoming register (no move); the SECOND
// materializes its parameter from r31 (`mr r3,r31`); the return combines from the saved registers
// (`add r3,r30,r31`). This fuses the two-call sequence and the two-param combine (fire 591). `*` is
// excluded (its latency reschedules the two-GPR epilogue restores) — deferred.
int g(int);
int h(int);
int seq_add(int a, int b)      { g(a); g(b); return a + b; }   // add r3,r30,r31
int seq_sub(int a, int b)      { g(a); g(b); return a - b; }   // subf r3,r31,r30
int seq_and(int a, int b)      { g(a); g(b); return a & b; }
int seq_commuted(int a, int b) { g(a); g(b); return b + a; }   // operand order still add r3,r30,r31
int seq_diff_callees(int a, int b) { g(a); h(b); return a | b; }
