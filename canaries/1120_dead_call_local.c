// A DEAD local whose initializer is a call (`int x = g();`, x never read): mwcc keeps the call for its
// side effect and discards the result. Ours previously KEPT it as a local, and the callee-saved combine
// path — which emits only statements + the return, not local initializers — silently DROPPED the call
// (a miscompile: it computed a+b with no `bl g`, and mis-assigned volatile r5/r6 as "saved"). Now the
// dead call-local is hoisted to a leading expression statement, so `int x=g(); return a+b;` compiles
// identically to `g(); return a+b;`. (fire 593)
int g(void);
int gi(int);
int discard_then_combine(int a, int b) { int x = g();  return a + b; }  // mr r31,b; mr r30,a; bl g; add r3,r30,r31
int discard_then_return(int a)         { int x = g();  return a;     }  // mr r31,a; bl g; mr r3,r31
void discard_void(int a)               { int x = gi(a);              }  // bl gi (a in r3), result discarded
int discard_two_in_order(int a)        { int y = g(); int x = g(); return a; }  // both calls run, a preserved
