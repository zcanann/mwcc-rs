/* Fire 372: STATIC CONST DOUBLE coefficient tables (the k_tan/s_atan
 * T[] vein). Constant-index reads load off ONE lis/addi ADDR16 base
 * (r3, .rodata local symbol): lis at slot 0, the low addi right after
 * the first float instruction — or directly after the lis when the
 * first scheduled node is itself a table read. Table reads emit after
 * an arith's pool constants. Deferred until fitted: an arith mixing a
 * pool and a table constant (register tie), multi-local table shapes
 * (s_atan's z/w). */
static const double T[] = { 1.5, 2.5, 3.5, 4.5, 5.5 };

double table_single(double x)
{
	return x * T[1];
}

double table_chain(double x)
{
	double z;

	z = x * x;
	return x + z * (T[1] + z * T[3]);
}

double table_dead_int(double x, int iy)
{
	double z;

	z = x * x;
	return x + z * (T[1] + z * (T[2] + z * T[4]));
}
