// A local declaration may open with a storage-class keyword. `register` and `auto` are
// ordinary-automatic hints with no codegen effect — mwcc compiles `register int x = e;` exactly like
// `int x = e;` — so the parser now accepts them (they are `Identifier` tokens ahead of the type) and
// lowers them as plain locals. `static` gives the variable static storage (a `<name>$N` object in
// `.sdata`/`.sbss`, codegen'd like a global); that path is not built yet, so a `static` local is
// recorded (LocalDeclaration.is_static) and DEFERRED — never mis-treated as an automatic.
//
// This canary exercises the byte-exact `register`/`auto` forms. (The deferred `static` case has no
// object to compare, so it is not included here.)
int reg_scalar(int a)       { register int x = a + 1; return x * 2; }
int auto_scalar(int a)      { auto int x = a - 1; return x; }
int reg_mixed(int a, int b) { register int t = a * b; return t + t; }
