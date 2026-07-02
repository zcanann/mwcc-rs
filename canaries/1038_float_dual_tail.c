/* The DUAL-TAIL float return (fire 349, the k_sin iy split's simple form):
 * the parser's guard+trailing-return normalization, cmpwi + branch, two
 * INDEPENDENT float DAG tails each ending in blr; the if pair + else-join
 * consume 3 @N; plain fadd and const-factor fmul (const in A) join the
 * vocabulary. */
double dual_simple(double x, int c)
{
	if (c == 0) {
		return x + 1.5;
	} else {
		return x * 2.5;
	}
}

double dual_truthy(double x, int c)
{
	if (c) {
		return x + 11.5;
	} else {
		return x * 12.5;
	}
}

double dual_deep(double x, int c)
{
	if (c == 0) {
		return x * (21.5 + x * 22.5);
	} else {
		return x * (23.5 + x * 24.5);
	}
}

double plain_fadd(double x)
{
	return x + 31.5;
}

double const_fmul(double x)
{
	return x * 32.5;
}

double fnmsub_of_chain(double x, double y)
{
	return x - (y * (41.5 + y * 42.5));
}
