/* The WINDOW-TOP tier (fire 340): named double locals take descending homes
 * from the window top (z f4, v f3) while the chains churn the low
 * registers; a param dying into a local's product vacates its register
 * (z = x*y claims y's f2); the deep mul-of-mul's cross-chain product joins
 * the tier structurally. */
double local_z2(double x)
{
	double z = x * x;
	return z * (1.5 + z * 2.5);
}

double local_z3(double x)
{
	double z = x * x;
	return z * (11.5 + z * (12.5 + z * 13.5));
}

double local_zv(double x)
{
	double z = x * x;
	double v = z * x;
	return x + v * (21.5 + z * 22.5);
}

double local_xy(double x, double y)
{
	double z = x * y;
	return z * (31.5 + z * 32.5);
}

double mul_of_mul_deep(double z, double w)
{
	return (z * w) * (41.5 + z * (42.5 + z * 43.5));
}
