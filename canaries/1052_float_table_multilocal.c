/* Fire 374: the SHALLOW multi-local table class (z, w = z*z with one
 * chain link per parity — the s_atan pattern's smallest member). The
 * consumer-boundary door joins the even chain to the return's f1 at
 * its death; the base PAIR (lis+addi together) lands after the first
 * float instruction when TWO locals lead (a single local splits them
 * around it). The deeper interleave (s_atan's full split) still
 * defers: its load-block permutation is unfitted. */
static const double aT[] = { 1.5, 2.5, 3.5 };

double table_two_locals(double x)
{
	double z, w;

	z = x * x;
	w = z * z;
	return z * (aT[0] + w * aT[2]) + w * aT[1];
}
