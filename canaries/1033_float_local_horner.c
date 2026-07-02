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

double local_zv_deep(double x)
{
	double z = x * x;
	double v = z * x;
	return x + v * (51.5 + z * (52.5 + z * 53.5));
}

double local_z4(double x)
{
	double z = x * x;
	return z * (61.5 + z * (62.5 + z * (63.5 + z * 64.5)));
}

double local_z4_xy(double x, double y)
{
	double z = x * y;
	return z * (71.5 + z * (72.5 + z * (73.5 + z * 74.5)));
}

double local_zv_deeper(double x)
{
	double z = x * x;
	double v = z * x;
	return x + v * (81.5 + z * (82.5 + z * (83.5 + z * 84.5)));
}

double local_z4_wfactor(double x, double w)
{
	double z = x * x;
	return z * (91.5 + z * (92.5 + z * (93.5 + z * w)));
}
