void setret(int *p, int a){ if (a) { *p = 1; return; } *p = 2; }
int pickret(int a, int b, int c){ if (a) { return b; } return c; }
