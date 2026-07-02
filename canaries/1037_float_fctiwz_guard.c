/* The NESTED fctiwz guard (fire 348, the full k_sin prefix): frame 32 with
 * a second conversion slot; bge TAIL / fctiwz f0,f1 / stfd 16 / lwz 20 /
 * cmpwi / bne TAIL / b EPILOGUE; the tail RELOADS x from the frame (the
 * multi-consumer reload joins the tier by death-asc order; a single-consumer
 * reload feeding z allocates ascending); @N = 4 if-labels + the conversion
 * + one post-pool number before extab. */
double fctiwz_flat(double x)
{
	int ix;
	ix = *(int*)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	return x * (1.5 + x * 2.5);
}

double fctiwz_z3(double x)
{
	int ix;
	double z;
	ix = *(int*)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	z = x * x;
	return z * (11.5 + z * (12.5 + z * 13.5));
}

double fctiwz_zv(double x)
{
	int ix;
	double z, v;
	ix = *(int*)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		if ((int)x == 0) {
			return x;
		}
	}
	z = x * x;
	v = z * x;
	return x + v * (21.5 + z * 22.5);
}
