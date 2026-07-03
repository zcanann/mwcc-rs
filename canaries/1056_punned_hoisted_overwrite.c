/* Fire 388: the HOISTED-ELSE OVERWRITE. `if (j0 cmp K) p = C1; else
   p = C2;` with BOTH arms nonzero constants branches (no if-conversion):
   the else value pre-loads into the home before the compare, the then
   arm overwrites under an inverted skip branch. Homes obey the liveness
   rule — the pre-loaded else value crosses the r0 write, so the home is
   r4 when the -K0 fold gives the guard a home (r3) and r3 when the
   extract goes straight to r0 (no fold). @N: +3 per diamond, the same
   count as the if-converted select. All six comparison ops. */
double hoist_less(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 < 20) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double hoist_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 == 20) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double hoist_greater(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 > 51) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double hoist_not_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 != 20) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double hoist_less_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 <= 20) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double hoist_greater_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 >= 20) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double hoist_no_fold(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = (i0 >> 20) & 0x7ff;
	if (j0 < 20) {
		i0 = 3;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
