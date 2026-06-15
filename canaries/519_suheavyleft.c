// The mirror — heavy subtree on the left — compiles identically (the order is by
// register need, not source position): ((c+d)*(e+g))*(a+b).
int suheavyleft(int a, int b, int c, int d, int e, int g){ return ((c + d) * (e + g)) * (a + b); }
