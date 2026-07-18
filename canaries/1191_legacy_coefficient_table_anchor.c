/* Coefficient-table relocation identity matrix. The instruction bytes are the
 * same whether the ADDR16 base pair binds the named LOCAL table object or the
 * `...rodata.0` section anchor; this canary pins that dead integer parameters do
 * not affect the boundary, while the two-kept-local DAG does. */
static const double legacy_T[] = { 1.5, 2.5, 3.5, 4.5 };

double ct_no_local(double x)
{
	return x * legacy_T[1];
}

double ct_dead_int(double x, int unused)
{
	return x * legacy_T[1];
}

double ct_two_dead_ints(double x, int unused1, int unused2)
{
	return x * legacy_T[1];
}

double ct_one_local(double x)
{
	double z;
	z = x * x;
	return x + z * (legacy_T[1] + z * legacy_T[3]);
}

double ct_one_local_dead_int(double x, int unused)
{
	double z;
	z = x * x;
	return x + z * (legacy_T[1] + z * legacy_T[3]);
}

double ct_two_locals(double x)
{
	double z, w;
	z = x * x;
	w = z * z;
	return z * (legacy_T[0] + w * legacy_T[2]) + w * legacy_T[1];
}
