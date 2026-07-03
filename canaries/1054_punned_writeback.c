/* Fire 380: the PUNNED-GUARD WRITEBACK (the s_floor tail). Punned int
 * reads of the double param spill it, a guard block mutates the punned
 * locals in scratch registers (r0, then the freed condition register —
 * which is why the cmpwi HOISTS above the stfd), the block writes them
 * back, and the double reloads. */
double writeback_one(double x, int c)
{
	int i0;

	i0 = *(int *)&x;
	if (c) {
		i0 = 0;
	}
	*(int *)&x = i0;
	return x;
}

double writeback_two(double x, int c)
{
	int i0, i1;

	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	if (c) {
		i0 = 0;
		i1 = 0;
	}
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}

double writeback_early_return(double x, int j0)
{
	int i0;

	i0 = *(int *)&x;
	if (j0 < 20) {
		if ((i0 & 3) == 0) {
			return x;
		}
		i0 &= 7;
	}
	*(int *)&x = i0;
	return x;
}

double writeback_computed_guard(double x)
{
	int i0, j0;

	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 < 20) {
		i0 = 0;
	}
	*(int *)&x = i0;
	return x;
}

double writeback_shift_guard(double x)
{
	int i0, j0;

	i0 = *(int *)&x;
	j0 = (i0 >> 20) - 5;
	if (j0 < 20) {
		i0 = 0;
	}
	*(int *)&x = i0;
	return x;
}

static const double huge = 1.0e300;

double writeback_float_guard(double x)
{
	int i0;

	i0 = *(int *)&x;
	if (huge + x > 0.0) {
		i0 = 0;
	}
	*(int *)&x = i0;
	return x;
}

double writeback_nested_guard(double x)
{
	int i0;

	i0 = *(int *)&x;
	if (huge + x > 0.0) {
		if (i0 >= 0) {
			i0 = 0;
		}
	}
	*(int *)&x = i0;
	return x;
}

double writeback_else_if(double x)
{
	int i0, i1;

	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	if (huge + x > 0.0) {
		if (i0 >= 0) {
			i0 = 0;
			i1 = 0;
		} else if (((i0 & 0x7fffffff) | i1) != 0) {
			i0 = 0xbff00000;
			i1 = 0;
		}
	}
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
/* Fire 388: the TOP-LEVEL LADDER (s_floor's shape). A multi-read guard
   takes the full value in its home (addi r3,r3,-1023) and every
   condition reads it plainly; the walker's else-if chain runs at the
   top level with join=writeback. i0 keeps r0 per the liveness rule (no
   fold, so the r0 scratch is never written). @N: +1 over the formula
   for the laddered outer. */
double writeback_ladder(double x)
{
	int i0, j0;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	if (j0 < 20) {
		i0 = 0;
	} else if (j0 > 51) {
		i0 = 5;
	} else {
		i0 &= 0x7ff;
	}
	*(int *)&x = i0;
	return x;
}
