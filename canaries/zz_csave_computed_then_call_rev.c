extern int g(int v);
int f(int a) { int x = a * 3 + 1; int y = g(x); return x + y; }
