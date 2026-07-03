/* Fire 358: the composed dual's chain-depth ENVELOPE. A const-bearing
 * shared chain claims only at the measured depths — standalone exactly
 * 3 ariths, in-frame 3..=4 (depth-4 standalone and depth-5 in-frame
 * schedules interleave the chain to cap the live window; both DEFER).
 * This pins the in-frame depth-3 member (k_sin is the depth-4 pin). */
static const double
C1 = 4.16666666666666019037e-02,
C2 = -1.38888888888741095749e-03;

double inframe_depth3(double x, double y, int iy)
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
	if (iy == 0) {
		return x + v * (3.5 + z * r);
	} else {
		return x - ((z * (0.5 * y - v * r) - y) - v * 4.5);
	}
}
