/* The fnmsub vocabulary (fire 338): `b - x*y` contracts to fnmsub with the
 * fmadd fixtures' exact geometry — single ops, alternating-sign horner
 * chains, mixed-sign chains, and the pooled-constant single fmadd. */
double fnmsub_single(double z)
{
	return 1.5 - z * 2.5;
}

double fnmsub_chain3(double z)
{
	return z * (11.5 - z * (12.5 - z * 13.5));
}

double fnmsub_chain4(double z)
{
	return z * (21.5 - z * (22.5 - z * (23.5 - z * 24.5)));
}

double fnmsub_wmul(double z, double w)
{
	return w * (31.5 - z * (32.5 - z * 33.5));
}

double mixed_signs(double z)
{
	return z * (41.5 + z * (42.5 - z * 43.5));
}

double single_fmadd_const(double z, double w)
{
	return z * w + 51.5;
}
