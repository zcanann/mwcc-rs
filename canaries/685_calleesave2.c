// Two parameters live across a call: each goes into a callee-saved register,
// assigned by parameter order — the last parameter takes r31, the first r30.
int g(void);
int calleesave2(int a, int b){ g(); return a + b; }
