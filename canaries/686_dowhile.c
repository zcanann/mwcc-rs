// A `do … while(--n)` counter loop: the counter lives in the callee-saved r31,
// the body branches back, and the decrement-and-test is a single addic./bne.
// The first matching loop.
int g(void);
void dowhile(int n){ do { g(); } while (--n); }
