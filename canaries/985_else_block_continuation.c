// An else-chain whose final else is a STATEMENT BLOCK (the fdlibm trig-dispatch shape):
// every prior branch returns, so the else block is simply the continuing body. The
// parser splices the block's statements into the ordered statement list after the
// migrated guards; these shapes then reduce to the verified early-return folds.
// (Previously `expected KeywordReturn` -- the guards loop demanded `else return`.)
int else_block(int a, int b)       { if (a) return 1; else { b = b + 1; } return b; }
int chain_else_block(int a, int b) { if (a) return 1; else if (a > 4) return 2; else { b = b * 2; } return b; }
