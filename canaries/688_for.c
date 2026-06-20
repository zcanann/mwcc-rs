// A counting `for (i = 0; i < n; i++)` loop: the counter in r31 (init 0), the
// bound in r30, body branches back via cmpw/blt. The counter is passed to the call.
int g(int);
void forloop(int n){ int i; for (i = 0; i < n; i++) g(i); }
