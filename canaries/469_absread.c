// flags: -sdata 0 -sdata2 0
// Absolute (-sdata 0) addressing: a global read materializes its address into
// the destination GPR (lis;addi;lwz 0) — base==dest, nothing folds.
extern int g;
int absread(void){ return g; }
