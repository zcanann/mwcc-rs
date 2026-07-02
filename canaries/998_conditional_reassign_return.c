/* A parameter conditionally reassigned (optionally after one global store), then
 * returned — the in-place diamond. mwcc keeps the parameter in its incoming
 * register through both paths; the merge is `mr r3,v`, and when v already lives
 * in r3 the merge is empty and the skip branch folds to `b<!c>lr`. Longer
 * then-bodies reschedule (a second store sinks below the addi) and defer. */

extern int g;

/* v == r3: the conditional-return fold (cmpw; bgelr; mr r3,r4; blr). */
int clamp_low(int a, int floor_value)
{
	if (a < floor_value) {
		a = floor_value;
	}
	return a;
}

/* v == r3 with a store: cmpwi; blelr; stw r3; addi r3,r3,-1; blr. */
int step(int a)
{
	if (a > 0) {
		g = a;
		a = a - 1;
	}
	return a;
}

/* v == r4: the skip targets the merge (cmpw; bge M; addi; M: mr r3,r4; blr). */
int bump(int a, int b)
{
	if (a < b) {
		b = b + 1;
	}
	return b;
}

/* store + reassign-from-other, v == r4. */
int step2(int a, int b)
{
	if (a > 0) {
		g = b;
		b = a;
	}
	return b;
}

/* store only, value returned: fold + stw + blr. */
int keep(int a)
{
	if (a > 0) {
		g = a;
	}
	return a;
}

/* reassign-from-other with v == r4 (the pick-diamond as a param reassign). */
int swap_in(int a, int b)
{
	if (a < b) {
		b = a;
	}
	return b;
}

/* subtract arm, v == r3. */
int down(int a, int b)
{
	if (a >= b) {
		a = a - 1;
	}
	return a;
}

/* unsigned compare and add. */
unsigned int ubump(unsigned int a, unsigned int b)
{
	if (a < b) {
		b = b + 4;
	}
	return b;
}

/* third parameter (v == r5) with a store. */
int third(int a, int b, int c)
{
	if (b != c) {
		g = c;
		c = c + 3;
	}
	return c;
}
