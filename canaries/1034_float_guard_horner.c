/* The GUARD + float-DAG composition (fire 343): int-param guards fold to
 * cmpwi + bXXlr ahead of the float tail (the value already in f1); a
 * z-local's fmul hoists ABOVE the guard; each folded if advances @N by 2
 * ahead of the pooled constants. */
double guard_flag(double x, int c)
{
	if (c) {
		return x;
	}
	return x * (1.5 + x * 2.5);
}

double guard_less(double x, int c)
{
	if (c < 3) {
		return x;
	}
	return x * (11.5 + x * 12.5);
}

double guard_two(double x, int c, int d)
{
	if (c) {
		return x;
	}
	if (d < 2) {
		return x;
	}
	return x * (21.5 + x * 22.5);
}

double guard_local(double x, int c)
{
	double z = x * x;
	if (c) {
		return x;
	}
	return z * (31.5 + z * 32.5);
}

double guard_deep(double x, int c)
{
	if (c == 5) {
		return x;
	}
	return x * (41.5 + x * (42.5 + x * 43.5));
}
