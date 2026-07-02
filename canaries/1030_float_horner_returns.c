/* The FLOAT DAG return arm (fires 331-337): double multiply-add trees
 * through the frozen float scheduling contract (single FPU pipe, load port,
 * blocked-load stall + empty-cycle lift) and the hybrid float register
 * machine (reverse death-order allocation, boundary shares, pending-arith
 * block). Pool constants intern in SOURCE order while the lfd's emit in
 * schedule order. */
double horner2(double z)
{
	return z * (1.5 + z * 2.5);
}

double horner3(double z)
{
	return z * (11.5 + z * (12.5 + z * 13.5));
}

double horner4(double z)
{
	return z * (21.5 + z * (22.5 + z * (23.5 + z * 24.5)));
}

double horner5(double z)
{
	return z * (31.5 + z * (32.5 + z * (33.5 + z * (34.5 + z * 35.5))));
}

double horner3_wmul(double z, double w)
{
	return w * (41.5 + z * (42.5 + z * 43.5));
}

double horner4_wmul(double z, double w)
{
	return w * (51.5 + z * (52.5 + z * (53.5 + z * 54.5)));
}

double two_chains(double z, double w)
{
	return z * (61.5 + w * (62.5 + w * 63.5)) + w * (64.5 + w * 65.5);
}

double two_chains_shallow(double z, double w)
{
	return z * (71.5 + w * 72.5) + w * (73.5 + w * 74.5);
}
