/* Fire 387: the BRANCHLESS ZERO-SELECT. `if (j0 cmp K) p = A; else p = B;`
   with one arm 0 if-converts on 2.6 to mask algebra with no branches:
   mask = -(cond) via per-op carry/sign recipes, select via andc (zero in
   the then) or and (zero in the else). The guard extract computes in
   place on the load; </> stage K and its sign in r3/r4; the L4 self-mask
   arm keeps the load in r0 and weaves its rlwinm between the guard
   extract and the -K addi. @N: +3 per diamond, +4 with the compound
   self-mask arm. */
double select_less_zero_then(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 < 20) {
		i0 = 0;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double select_less_self_mask(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 < 20) {
		i0 = 0;
	} else {
		i0 &= 0x7ff;
	}
	*(int *)&x = i0;
	return x;
}
double select_less_zero_else(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 < 20) {
		i0 = 5;
	} else {
		i0 = 0;
	}
	*(int *)&x = i0;
	return x;
}
double select_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 == 20) {
		i0 = 0;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double select_not_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 != 20) {
		i0 = 0;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double select_greater(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 > 51) {
		i0 = 0;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
double select_less_equal(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 <= 20) {
		i0 = 0;
	} else {
		i0 = 5;
	}
	*(int *)&x = i0;
	return x;
}
