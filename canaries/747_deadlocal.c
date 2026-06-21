// A local that is never referenced and whose initializer has no side effect is dead
// — mwcc emits nothing for it. `int s = 0; return *p;` is just the load, no `li`.
// (A used local, a call-initialized one whose call must run, or an address-taken one
// is kept.)
int deadlocal_one(int *p)      { int s = 0; return *p; }
int deadlocal_const(int x)     { int unused = 5; return x; }
int deadlocal_two(int x)       { int a = 1; int b = 2; return x; }
