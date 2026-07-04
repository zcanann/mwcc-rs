extern int g(void);
int f(int a) { int x = g(); return x ^ a; }
