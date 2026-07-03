/* Fire 364: the DEPTH-5 in-frame dual — the k_cos chain length. The
 * frozen order model already produced the interleave (C1's load lands
 * after chain1, v after); the registers took three fitted rules: (1)
 * ADJACENCY — v joins the tier def-DESC when a load separates it from
 * the first chain arith (window 9: v f8, z f7, reload f6); (2) dual
 * LOADS allocate by death-DESC/start-DESC ASCENDING first-fit (the
 * post-chain C1 claims f0 for its long range early, so 3.5 shares f0
 * across disjoint ranges); (3) dying reuse is availability-checked —
 * chain1's MIN (f0) is blocked by C1's claim, landing on its factor's
 * f5. */
static const double
C1 = 4.16666666666666019037e-02,
C2 = -1.38888888888741095749e-03;

double inframe_depth5(double x, double y, int iy)
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
	r = C1 + z * (C2 + z * (1.5 + z * (2.5 + z * (3.5 + z * 4.5))));
	if (iy == 0) {
		return x + v * (5.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 6.5);
	}
}
