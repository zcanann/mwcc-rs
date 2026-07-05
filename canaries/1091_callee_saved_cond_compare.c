extern void side_effect(void);
int cond_cmp(int a, int b) { if (b > 3) side_effect(); return a; }
