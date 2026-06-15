// MSL_C arith.c abs(): if (n<0) return -n; else return n; -> the branchless abs
// idiom srawi t,x,31; xor r0,t,x; subf d,t,r0. if/else-return lowers to the
// ternary c?x:y.
int absval(int n) { if (n < 0) return -n; else return n; }
