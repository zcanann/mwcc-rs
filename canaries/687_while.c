// A `while(--n)` counter loop: like the do-while but the condition is tested
// first (an initial branch to the decrement-and-test).
int g(void);
void whileloop(int n){ while (--n) { g(); } }
