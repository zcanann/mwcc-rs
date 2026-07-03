/* The SHARED-DAG dual tail (fire 355, the real __kernel_sin composition):
 * leading double locals — register PRODUCTS (z, v, w) and the k_sin r
 * chain (>= 3 ariths; shallower const-bearing chains DEFER) — materialize
 * ONCE ahead of the cmpwi as a shared DAG with a STORE-sink carrying tail
 * liveness. The register machine runs in DUAL mode: tier def-DESC, the
 * UNION floor (locals + params read by either tail + max per-tail literal
 * count), the compare interleaved after the second shared load (or the
 * first op when loadless). Pool constants intern in SOURCE order. */
double dual_shared_z(double x, int c)
{
	double z;

	z = x * x;
	if (c == 0) {
		return z * (1.5 + z * 2.5);
	} else {
		return z * (3.5 + z * 4.5);
	}
}

double dual_shared_zv(double x, int c)
{
	double z, v;

	z = x * x;
	v = z * x;
	if (c == 0) {
		return x + v * (1.5 + z * 2.5);
	} else {
		return x - v * (3.5 + z * 4.5);
	}
}

double dual_shared_zvw(double x, int c)
{
	double z, v, w;

	z = x * x;
	v = z * x;
	w = v * z;
	if (c == 0) {
		return x + w * (1.5 + z * 2.5);
	} else {
		return x - w * (3.5 + v * 4.5);
	}
}

double dual_ksin(double x, double y, int iy)
{
	double z, r, v;

	z = x * x;
	v = z * x;
	r = 1.5 + z * (2.5 + z * (3.5 + z * 4.5));
	if (iy == 0) {
		return x + v * (5.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 6.5);
	}
}
