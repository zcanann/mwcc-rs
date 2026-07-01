// A constant-valued early-return guard mixed with local/parameter reassignment: mwcc
// folds the guard with the value-tracked fall-through into the trailing-guard select --
// `cmpwi; li r3,V; bnelr; <inlined tail>` -- IDENTICALLY whether the guard comes before
// or after the reassignments (the guard reads only never-reassigned parameters, so both
// orders read the same pristine registers). Previously the ordered form was a parse
// error and the flat form deferred ("value tracking combined with guards").
int guard_then_assign(int a, int b)  { if (a) return 1; b = b + 1; return b; }
int assign_then_guard(int a, int b)  { b = b + 1; if (a) return 1; return b; }
int two_guards_assign(int a, int b)  { if (a) return 1; if (a > 9) return 2; b = b * 2; return b; }
int compare_guard(int a, int b)      { if (a > 3) return -1; b = b + 4; return b; }
// NOTE: a tail that still reads the RESULT register's parameter (`b = b - a; return b`
// reads a in r3, which the fold's `li r3,V` clobbers) does not fold -- mwcc keeps a real
// early-return branch there. Deferred until early-return branch codegen.

// A REGISTER-valued guard does NOT fold order-independently (mwcc keeps a real forward
// branch in the ordered source, an inverted select in the flat one) -- both defer; only
// the assignment-free flat form is byte-exact today.
int flat_register_guard(int a, int b, int c) { if (a) return c; return b + c; }
