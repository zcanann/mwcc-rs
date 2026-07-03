/* Fire 360: the k_cos BIG-constant dual compare (`ix < 0x3FD33333`).
 * The constant materializes INSIDE the shared schedule — lis r3 + addi
 * r0 right after the x reload, cmpw ix,r0 after the FOURTH shared load
 * (measured at chain depths 3 and 4) — so the prefix SPLITS raw/masked
 * (lwz r3; clrlwi r4,r3,1) keeping ix clear of the lis. Less only, addi
 * low half <= 0x7fff, no int params. */
static const double
one = 1.00000000000000000000e+00,
C1 = 4.16666666666666019037e-02,
C2 = -1.38888888888741095749e-03;

double ix_dual_big(double x, double y)
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
	if (ix < 0x3FD33333) {
		return x + v * (4.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 5.5);
	}
}

double ix_dual_big_d4(double x, double y)
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
	if (ix < 0x3FD33333) {
		return x + v * (4.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 5.5);
	}
}
