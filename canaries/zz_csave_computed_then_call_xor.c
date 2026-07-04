extern int g(int v);
int f(int a) { int x = a * 9 + 4; int y = g(x); return y ^ x; }
