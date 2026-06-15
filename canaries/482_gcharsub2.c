// Two signed char globals under subtraction: loads batched (lbz;lbz) ahead of
// the sign-extensions, the right operand anchored for `subf` (left - right).
extern char a, b;
int gcharsub2(void){ return a - b; }
