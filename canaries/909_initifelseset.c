// `T b = INIT; if (c) b = A; else b = B; return b;` — both arms reassign b, so INIT is DEAD and the
// whole body is the select `return c ? A : B`. mwcc compiles the init form identically to the no-init
// form (verified). try_conditional_assign now allows a dead initializer (it builds the select purely
// from the arm values), so these fold to mwcc's branchless select like the existing no-init handler.
// (A no-ELSE `if(c)b=NEW; return b;` still routes to the initialized-handler's early-return branch
// form; an if-else with a COMPARISON condition is a separate pre-existing defer, unrelated.)
int sel_const(int a)            { int b = 0;  if (a) b = 1; else b = 2;  return b; }
int sel_const2(int a)           { int b = 5;  if (a) b = 10; else b = 20; return b; }
int sel_var_true(int a, int c)  { int b = 0;  if (a) b = c; else b = 1;  return b; }
int sel_var_false(int a, int c) { int b = 99; if (a) b = 1; else b = c;  return b; }
int sel_init_var(int a, int c)  { int b = c;  if (a) b = 1; else b = 2;  return b; }
