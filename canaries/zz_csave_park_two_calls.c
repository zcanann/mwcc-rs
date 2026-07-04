extern int g(int v);
int f(int a) { int x = g(a); int y = g(x); return y + a + x; }
