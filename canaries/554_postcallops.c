// Shift/unary/mask of a call result operate in place on the result register (r3),
// not bounced through the scratch — combined with the LR-reload hoist this matches
// mwcc's full post-call epilogue shape.
int g(int);
int dbl(int a){ return g(a) * 2; }
int neg(int a){ return -g(a); }
int low(int a){ return g(a) & 0xff; }
