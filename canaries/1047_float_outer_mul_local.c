/* Fire 363: the OUTER-MUL chain local (k_cos's r = z*(C1+z*(...))) — a
 * Mul-rooted const chain in the in-frame dual, its root on MIN-dying,
 * with SINGLE-FMADD literal-free tails over pseudo-params (the pseudo
 * tail has no other claim path, so the 1-arith no-literal gate opens
 * for it). The fmadd-family-rooted sibling with literal-free tails
 * DEFERS (root register rule unfitted: MIN-dying here, C-operand in
 * the k_sin literal-tail class). */
static const double
C1 = 4.16666666666666019037e-02,
C2 = -1.38888888888741095749e-03;

double outer_mul_local(double x, double y, int iy)
{
	double z, r;
	int ix;

	ix = *(int *)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	z = x * x;
	r = z * (C1 + z * (C2 + z * (1.5 + z * 2.5)));
	if (iy == 0) {
		return x + z * r;
	} else {
		return x - z * r;
	}
}
