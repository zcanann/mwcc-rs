/* Fire 366: THE ESCAPING-ROOT RULE (found by causal probes, fire 365).
 * The non-tier chain root allocates ASCENDING, skipping registers each
 * tail claims BEFORE the root's last read on that path plus everything
 * living BEYOND the root's slot (dying AT it is reusable). Both tails
 * dry-run with the root as a high placeholder pseudo (f30), the claims
 * harvest from the emitted instructions, and the register fixes before
 * the shared emission. This one rule subsumes the old C-operand and
 * MIN-dying picks (r = f0 with first-op reads, f1 in the k_sin class,
 * f3 under the k_cos then-tail's x*y/0.5/one claims) and opens both
 * the literal-free-tail class and the k_cos 6-const outer-mul chain
 * with its fsub-of-fsub then-tail. */
static const double
one = 1.00000000000000000000e+00,
C1 = 4.16666666666666019037e-02,
C2 = -1.38888888888741095749e-03,
C3 = 2.48015872894767294178e-05,
C4 = -2.75573143513906633035e-07,
C5 = 2.08757232129817482790e-09,
C6 = -1.13596475577881948265e-11;

double kcos_minus_diamond(double x, double y)
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
	r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
	if (ix < 0x3FD33333) {
		return one - (0.5 * z - (z * r - x * y));
	} else {
		return x + z * r;
	}
}

double zr_literal_free(double x, double y, int iy)
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
	r = C1 + z * (C2 + z * (1.5 + z * 2.5));
	if (iy == 0) {
		return x + z * r;
	} else {
		return x - z * r;
	}
}
