// A reassigned PARAMETER is a register-resident variable, just like a local: `a = expr` mutates
// the param's register in place and the value tracker inlines it into the return. The parser now
// emits an Assign (not a Store) for `param = value`, and the value tracker takes over a no-locals
// body when it sees that Assign. So `int f(int a){ a += 5; return a; }` is just `addi r3,r3,5` —
// the reassignment folds into `return a + 5`. A const reassign (`a = 5`) drops to `li`; a copy
// (`a = b`) returns the other param. Globals stay Stores (observable), so `g = 5` is unaffected.
// A param reassign mixed with a call, an `if`, or a memory store defers (the straight-line path
// rejects a stray Assign rather than miscompiling).
int addassign(int a)            { a += 5; return a; }
int reassign(int a)             { a = a + 1; return a; }
int constassign(int a)          { a = 5; return a; }
int mulassign(int a)            { a *= 2; return a; }
int andassign(int a)            { a &= 255; return a; }
int twoparam(int a, int b)      { a += b; return a; }
int copyparam(int a, int b)     { a = b; return a; }
unsigned shrassign(unsigned a)  { a >>= 2; return a; }
int incparam(int a)             { a++; return a; }
void deadassign(int a)          { a += 5; }
