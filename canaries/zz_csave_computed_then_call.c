extern int g(int v);
int f(int a) { int x = a * 5 + 2; int y = g(x); return y + x; }
