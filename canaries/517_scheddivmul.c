// Divide and multiply scheduled together: the latency policy (divide > multiply)
// orders them as mwcc does. (a/b)+(c*d) is byte-exact through the scheduler.
int scheddivmul(int a, int b, int c, int d){ return (a / b) + (c * d); }
