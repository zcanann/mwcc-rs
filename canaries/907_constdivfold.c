// A constant `/` or `%` of two integer literals folds to the result (`li r3,N`), like const +/-/*
// already did — previously a runtime divide sequence. Runtime division (a variable operand) is
// unchanged. Folded in the expression parser when both operands are integer literals.
int div_exact(void)  { return 16 / 4; }    // 4
int div_trunc(void)  { return 100 / 7; }   // 14
int div_one(void)    { return 10 / 1; }    // 10
int div_big(void)    { return 1000 / 3; }  // 333
int mod_op(void)     { return 17 % 5; }     // 2
int mod_zero(void)   { return 20 % 4; }     // 0
