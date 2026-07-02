/* The subnormal writeback block: a guard-style skip over a float multiply
 * written back to the parameter's slot, the merge reloading x unconditionally
 * (the slot is written). The test's lis hoists exactly like a guard's; the
 * multiply reads the still-in-f1 parameter with the pooled constant as frC
 * (lfd f0; fmul f0,f1,f0; stfd f0,8(r1); MERGE: lfd f1,8(r1)). */

static const double two54 = 1.80143985094819840000e+16;

double m1(double x)
{
	if (*(int*)&x < 0x00100000) {
		x *= two54;
	}
	return x;
}

/* the same through a punned local. */
double m1b(double x)
{
	int hx = *(int*)&x;
	if (hx < 0x00100000) {
		x *= two54;
	}
	return x;
}
