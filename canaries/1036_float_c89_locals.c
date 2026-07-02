/* C89 fdlibm locals (fire 347): uninitialized declarations with leading
 * Assign statements normalize into initializers (assignment order = the
 * tier's definition order), alternating with the guard hoist through the
 * evaluate_body recursion. */
double c89_z(double x)
{
	double z;
	z = x * x;
	return z * (1.5 + z * 2.5);
}

double c89_zvr(double x)
{
	double z, r, v;
	z = x * x;
	v = z * x;
	r = 11.5 + z * (12.5 + z * (13.5 + z * (14.5 + z * 15.5)));
	return x + v * (16.5 + z * r);
}

double c89_punned(double x)
{
	int ix;
	ix = *(int*)&x;
	if (ix < 0) {
		return x;
	}
	return x * (21.5 + x * 22.5);
}

double c89_punned_guards(double x, int iy)
{
	int ix;
	ix = *(int*)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		return x;
	}
	if (iy) {
		return x;
	}
	return x * (31.5 + x * 32.5);
}
