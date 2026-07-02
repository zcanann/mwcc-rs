/* `T1 || T2` inside ONE guard — the disjunction topology: the first test
 * branches INTO the value block on TRUE, the second skips past it; one shared
 * value block falls into the epilogue; the label counter advances 3 (two
 * tests, one value).
 *
 * All loads come first (a second word rides the first's load latency in r4 —
 * r0 holds the hoisted lis constant), the mask after, then the tests. */

/* frexp's exact guard: lis-compare || or.-zero over the same masked word. */
double or_top(double x)
{
	int hx = *(int*)&x;
	int ix = hx & 0x7fffffff;
	int lx = *(1 + (int*)&x);
	if (ix >= 0x7ff00000 || ((ix | lx) == 0))
		return 0.5;
	return x;
}

/* two lis-compares of one shared unmasked word: second lis inline. */
double or_two_cmp(double x)
{
	int hx = *(int*)&x;
	if (hx >= 0x7ff00000 || hx < 0x00100000)
		return 0.5;
	return x;
}
