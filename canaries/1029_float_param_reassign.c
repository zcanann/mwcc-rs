/* A float param reassigned in a branch stays IN ITS PARAM REGISTER — an
 * in-place fneg behind the branch (measured; `double t = x; if (c) t = -x;`
 * canonicalizes identically, the bare-copy local aliasing the param). */
double param_reassign(double x, int c)
{
	if (c) {
		x = -x;
	}
	return x * 2.0;
}

double alias_form(double x, int c)
{
	double t = x;
	if (c) {
		t = -x;
	}
	return t * 2.0;
}

double two_flips(double x, int c, int d)
{
	if (c) {
		x = -x;
	}
	if (d > 2) {
		x = -x;
	}
	return x * 2.0;
}
