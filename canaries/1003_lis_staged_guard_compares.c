/* Guard compares against lis-able constants (low half zero, too big for a
 * cmpwi immediate) over punned words of a double parameter.
 *
 * RELATIONS stage the constant with `lis r0,HI` HOISTED into the prologue
 * latency slot (between stwu and the stfd spill); the word loads into r3 (r0
 * is taken), an optional & 0x7fffffff folds in place (`clrlwi r3,r3,1`), and
 * a register `cmpw r3,r0` feeds the skip branch.
 *
 * EQUALITY folds the constant instead: no lis — `addis r0,r3,-HI` then
 * `cmplwi r0,0` feeding beq/bne. */

/* the frexp inf/nan test: masked word >= 0x7ff00000. */
double big_guard(double x)
{
	int hx = *(int*)&x;
	int ix = hx & 0x7fffffff;
	if (ix >= 0x7ff00000)
		return 0.0;
	return x;
}

/* the frexp subnormal test: unmasked word < 0x00100000. */
double lowmask_guard(double x)
{
	int hx = *(int*)&x;
	if (hx < 0x00100000)
		return 0.0;
	return x;
}

/* strict greater on the low word. */
double gt_lo(double x)
{
	int lx = *(1 + (int*)&x);
	if (lx > 0x00200000)
		return 0.25;
	return x;
}

/* equality: addis-fold, no lis. */
double eq_big(double x)
{
	int hx = *(int*)&x;
	if (hx == 0x40000000)
		return 2.0;
	return x;
}

/* inequality: the same fold, inverted branch. */
double ne_big(double x)
{
	int lx = *(1 + (int*)&x);
	if (lx != 0x00100000)
		return 0.125;
	return x;
}
