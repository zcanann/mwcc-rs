/* Locals live ACROSS branches (the s_atan id/x skeleton): the condition's
 * cmpwi leads; inits compute SPECULATIVELY before the branch; every
 * definition of one local shares ONE register home — r0 first unless a use
 * forbids it (an addi source), else the condition's DYING register, else a
 * free volatile; the trailing return consumes the locals as pseudo-params.
 * mwcc canonicalizes init-then-reassign and if/else-init identically. */
int live_reassign(int a, int c)
{
	int t = a + 1;
	if (c) {
		t = a * 3;
	}
	return t + 2;
}

int two_live(int a, int b, int c)
{
	int t = a + 1;
	int u = b + 2;
	if (c) {
		t = a * 3;
		u = b * 5;
	}
	return t + u;
}

int cond_still_live(int a, int c)
{
	int t = a + 1;
	if (c > 5) {
		t = a * 3;
	}
	return t + c;
}

int two_branches(int a, int c, int d)
{
	int t = a + 1;
	if (c) {
		t = a * 3;
	}
	if (d) {
		t = t + 7;
	}
	return t;
}
