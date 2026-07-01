// A CALL in a non-first argument clobbers the argument registers already holding earlier arguments (a
// call returns in r3 and clobbers r3-r12) and lands its own result in r3, not the argument's
// positional register. Evaluating arguments left-to-right therefore MISCOMPILED `s(5, f())` /
// `s(f(), g())` (the later call overwrote the earlier argument). mwcc evaluates such arguments
// right-first, preserving earlier results in callee-saved registers — not modeled yet, so those defer.
//
// This canary pins the still-byte-exact shapes: no call, a single call in the FIRST argument followed
// by non-clobbering constants, or a lone call argument. (The deferred multi-call-argument cases have
// no object to compare, so they are not included.)
int  f(void);
int  g(int);
void s1(int);
void s2(int, int);
void s3(int, int, int);
void lone_call(void)     { s1(f()); }          // bl f; bl s1
void call_then_const(void) { s2(f(), 5); }     // bl f; li r4,5; bl s2   (const does not clobber r3)
void call_then_consts(void){ s3(f(), 1, 2); }  // bl f; li r4,1; li r5,2; bl s3
void nested_single(int x){ s1(g(x)); }         // bl g; bl s1
void plain_args(int a, int b) { s2(a, b); }    // no calls in the arguments
