/* The record-form OR guard and two-guard chains over punned words — the last
 * control pieces of the frexp skeleton.
 *
 * `(a | b) == 0` loads BOTH words first (the second fills the first's load
 * latency), masks after, then `or. r0,r3,r0` sets CR0 with no compare.
 *
 * A guard CHAIN shares one loaded word down the chain; only the FIRST guard's
 * lis-staged constant hoists into the prologue latency slot (a later guard
 * materializes its lis inline); a non-final guard's value takes `b` to the
 * shared epilogue, the final one falls into it; the label counter advances 2
 * per guard. */

/* plain or-zero: lwz; lwz; or.; bne. */
double or_zero(double x)
{
	int ix = *(int*)&x;
	int lx = *(1 + (int*)&x);
	if ((ix | lx) == 0)
		return 0.5;
	return x;
}

/* the frexp form: masked high word — loads first, clrlwi after both. */
double or_mask(double x)
{
	int hx = *(int*)&x;
	int ix = hx & 0x7fffffff;
	int lx = *(1 + (int*)&x);
	if ((ix | lx) == 0)
		return 0.5;
	return x;
}

/* inverted: beq skip. */
double or_ne(double x)
{
	int ix = *(int*)&x;
	int lx = *(1 + (int*)&x);
	if ((ix | lx) != 0)
		return 1.25;
	return x;
}

/* the frexp guard pair as a chain: shared lwz r3, first lis hoisted, second
 * inline; first value takes b to the epilogue. */
double two_guards(double x)
{
	int hx = *(int*)&x;
	if (hx >= 0x7ff00000)
		return 0.0;
	if (hx < 0x00100000)
		return 0.5;
	return x;
}

/* mixed idioms in one chain: addis-fold equality, then a lis compare. */
double chain_eq(double x)
{
	int hx = *(int*)&x;
	if (hx == 0x7ff00000)
		return 0.0;
	if (hx >= 0x00200000)
		return 0.5;
	return x;
}
