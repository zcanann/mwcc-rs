/* Fire 359: the k_cos-family dual split on the PRESERVED punned ix.
 * The trailing if/else may compare the prefix's ix local against an i16
 * literal — ix stays live in the prefix's compare register (r3 with no
 * int params, r4 with one) and the dual's cmpwi reads it there. Only
 * the lis/cmpw prefix form qualifies (the r0 small-compare form would
 * clobber). Byte-identical to the iy-param dual otherwise (measured:
 * bne->bge is the whole text delta). */
static const double
one = 1.00000000000000000000e+00,
C1 = 4.16666666666666019037e-02,
C2 = -1.38888888888741095749e-03;

double ix_dual_lt(double x, double y)
{
	double z, v, r;
	int ix;

	ix = *(int *)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	z = x * x;
	v = z * x;
	r = C1 + z * (C2 + z * (1.5 + z * 2.5));
	if (ix < 0x100) {
		return x + v * (3.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 4.5);
	}
}

double ix_dual_gt_early_const(double x, double y)
{
	double z, v, r;
	int ix;

	ix = *(int *)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return one;
		}
	}
	z = x * x;
	v = z * x;
	r = C1 + z * (C2 + z * (1.5 + z * (2.5 + z * 3.5)));
	if (ix > 0x200) {
		return x + v * (4.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 5.5);
	}
}
