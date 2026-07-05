extern void side_effect(void);
int cond_call(int a, int b) { if (b) side_effect(); return a; }
