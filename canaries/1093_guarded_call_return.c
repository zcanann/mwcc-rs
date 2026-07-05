extern int compute(int x);
int guarded_call(int a) { if (a) return compute(a); return 0; }
