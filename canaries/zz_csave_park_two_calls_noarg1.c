extern int g(void);
extern int h(int v);
int f(int a) { int x = g(); int y = h(x); return y + a + x; }
