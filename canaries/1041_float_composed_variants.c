/* Fire 357: k_cos prerequisites + two latent wrong-byte classes killed.
 * (1) The nested punned prefix's early return of a FOLDED STATIC CONST
 *     (`return one;`) pools and loads into f1 ahead of the epilogue
 *     branch — dual and single tails.
 * (2) The register tier's equal-death tie orders by CONSUMER COUNT DESC,
 *     then local-before-load, then position (measured: the 2-const
 *     return-x tie z-over-reload; reload_zv's reload-over-v; the 5-const
 *     chain z-over-reload) — previously the 2-const and 5-const shapes
 *     emitted WRONG register assignments.
 * (A dup literal shared between the region and a tail now DEFERS.) */
static const double
one = 1.00000000000000000000e+00,
C1  = 4.16666666666666019037e-02,
C2  = -1.38888888888741095749e-03;

double early_const_dual(double x, double y, int iy)
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
	if (iy == 0) {
		return x + v * (4.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 5.5);
	}
}

double early_const_single(double x)
{
	double z, r;
	int ix;

	ix = *(int *)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return one;
		}
	}
	z = x * x;
	r = C1 + z * (C2 + z * (1.5 + z * (2.5 + z * 3.5)));
	return x + z * r;
}

double tie_short_chain(double x)
{
	double z;
	int ix;

	ix = *(int *)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	z = x * x;
	return x + z * (C1 + z * C2);
}

double tie_mid_chain(double x)
{
	double z;
	int ix;

	ix = *(int *)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	z = x * x;
	return x + z * (C1 + z * (C2 + z * 1.5));
}
