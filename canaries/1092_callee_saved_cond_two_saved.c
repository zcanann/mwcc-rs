extern void side_effect(void);
int cond_two(int a, int b, int c) { if (c) side_effect(); return a + b; }
