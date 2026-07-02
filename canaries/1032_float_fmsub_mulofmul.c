/* fmsub roots + the shallow mul-of-mul (fire 339): the root-slot order rule
 * (the return's B-slot operand claims f0), the dying-door share (z vacates
 * f1 through m1 to the root), and the chain-left fmul canonicalization. */
double fmsub_simple(double z)
{
	return z * (1.5 + z * 2.5) - 3.5;
}

double fmsub_deep(double z)
{
	return z * (11.5 + z * (12.5 + z * 13.5)) - 14.5;
}

double fmsub_wmul(double z, double w)
{
	return w * (21.5 + z * 22.5) - 23.5;
}

double fmsub_of_fnmsub(double z)
{
	return z * (31.5 - z * 32.5) - 33.5;
}

double mul_of_mul(double z, double w)
{
	return (z * w) * (41.5 + z * 42.5);
}

double mul_of_mul_flipped(double z, double w)
{
	return (51.5 + z * 52.5) * (z * w);
}

double mul_of_fnmsub(double z, double w)
{
	return (z * w) * (61.5 - z * 62.5);
}

double single_fmsub(double z, double w)
{
	return z * w - 71.5;
}
