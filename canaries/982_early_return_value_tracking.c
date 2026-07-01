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
// branch in the ordered source, an inverted select in the flat one).
int flat_register_guard(int a, int b, int c) { if (a) return c; return b + c; }

// The ordered early-return BRANCH form: a tail reading TWO-plus distinct parameters with
// a register guard value or a reads-r3 tail emits a real forward branch in the ORDERED
// source -- `cmpwi; beq CONT; mr r3,c; blr; CONT: add r3,r4,r5`.
int branch_register_value(int a, int b, int c) { if (a) return c; b = b + c; return b; }
int branch_reads_r3_tail(int a, int b)         { if (a > 3) return -1; b = b - a; return b; }

// The FLAT variants of those same bodies TEMP-FOLD instead: the tail computes into its
// home r0 first, then a conditional return -- `cmpwi; add r0,r4,r5; li r3,5; bnelr;
// mr r3,r0` (constant value, branch on the guard TAKEN) or `cmpwi; add r0,r4,r5;
// mr r3,r0; beqlr; mr r3,c` (register value, branch on the guard NOT taken). A two-
// parameter tail takes this form even when it does not read r3.
int temp_fold_const(int a, int b, int c)     { b = b + c; if (a) return 5; return b; }
int temp_fold_register(int a, int b, int c)  { b = b + c; if (a) return c; return b; }
int temp_fold_reads_r3(int a, int b)         { b = b - a; if (a > 3) return -1; return b; }
int temp_fold_one_param(int a, int b)        { b = a * 2; if (a > 3) return -1; return b; }

// And the ordered two-parameter CONST tail is the branch form, not the fold.
int branch_const_two_param(int a, int b, int c) { if (a) return 5; b = b + c; return b; }

// A register-valued guard over a ONE-parameter tail folds INVERTED, identically in both
// orders: the tail computes directly into r3, the conditional return fires when the
// guard is NOT taken, the guard value follows (`cmpwi; addi r3,r4,1; beqlr; mr r3,c`).
int inverted_fold_ordered(int a, int b, int c) { if (a) return c; b = b + 1; return b; }
int inverted_fold_flat(int a, int b, int c)    { b = b + 1; if (a) return c; return b; }
