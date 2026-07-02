/* The reversed guard form: the guard returns the PARAMETER, the fall-through a
 * literal.
 *
 * Plain (g1): the empty value block still gets its unconditional branch — no
 * condition inversion: `cmpwi; bge FALL; b EPI; FALL: lfd literal; EPI`.
 *
 * Disjunction (g2): the value block is a branch JOIN, so `return x` RELOADS
 * from the slot (`lfd f1,8(r1); b EPI`) even though x is unwritten; the second
 * test is the shared-word cmpwi (`cmpwi r3,C`), not an r0-staged compare. */

double g1(double x)
{
	int hx = *(int*)&x;
	if (hx < 0)
		return x;
	return 0.5;
}

double g2(double x)
{
	int hx = *(int*)&x;
	if (hx >= 0x7ff00000 || hx < 0)
		return x;
	return 0.5;
}
